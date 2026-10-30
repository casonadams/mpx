use crossterm::event::{Event, KeyEvent, KeyEventKind, MouseEventKind};
use std::collections::{HashMap, VecDeque};
use std::io::{BufWriter, Write, stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

use super::canvas::Canvas;
use super::command::Command;
use super::copy::CopyState;
use super::exec::default_shell;
use super::frame;
use super::keymap::{self, Keymap};
use super::overlay::Overlay;
use super::panes::{BorderGrab, LayoutCache, PaneRect, resized_elsewhere, sync_dims};
use super::prompt::PromptState;
use super::render::copy_to_clipboard;
use super::status::{local_utc_offset, restored_notice};
use super::terminal::TerminalGuard;
use crate::client::Client;
use crate::config::Config;
use crate::node::sleep_until_opt;
use crate::keys::TermModes;
use crate::layout::geometry::Rect;
use crate::protocol::{NewWindowOpts, NodeInfo, SpawnOpts, StreamEvent};
use crate::vt::VtScreen;

/// Initial screen when attaching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartView {
    Panes,
}

/// What the event loop does after handling one input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// Input state. The active handler takes the mode out and puts back the mode it stays in.
pub enum Mode {
    Normal,
    Prefix,
    Copy(CopyState),
    Overlay(Overlay),
    Prompt(PromptState),
    Confirm(String, Command),
}

/// Event-stream subscription for the attached session. Events tagged with an older
/// generation come from a previous session and are dropped.
pub struct Subscription {
    pub generation: u64,
    pub tx: mpsc::Sender<(u64, StreamEvent)>,
    pub task: Option<JoinHandle<()>>,
}

pub type StreamRx = mpsc::Receiver<(u64, StreamEvent)>;

/// Stream events the app holds before its subscription stops reading the daemon. Small, so a pane
/// flooding faster than the app draws is paced by the daemon rather than queued here, ahead of a
/// keystroke's echo in another pane.
const STREAM_QUEUE: usize = 64;

/// Window in which at most `FRAMES_PER_BUDGET` frames are drawn for pane output, so a flood cannot
/// starve input.
pub const FRAME_BUDGET: Duration = Duration::from_millis(16);
/// Output frames allowed per budget window: more than one, so a burst that arrives in a few reads
/// (a pager page) is never held back a whole window between its pieces.
pub const FRAMES_PER_BUDGET: usize = 4;
/// Output of the focused pane this soon after a keypress is likely its response (the terminal's
/// echo, then the program's answer): its frames are drawn at once, past the budget.
pub const TYPING_WINDOW: Duration = Duration::from_millis(100);
/// Frames per keypress drawn past the budget, so a flood started by a keypress (Enter on
/// `cat bigfile`) still cannot starve input.
pub const TYPING_FRAMES: u8 = 16;
/// Times output handling yields to let the rest of a burst arrive before its frame.
const BURST_YIELDS: usize = 4;
const REPEAT_WINDOW: Duration = Duration::from_millis(500);
const MESSAGE_TTL: Duration = Duration::from_secs(3);
const HISTORY_CAP: usize = 100;

pub struct App {
    pub endpoint: String,
    pub session: String,
    pub client: Client,
    /// The effective config: `base_config` with the session's `set-option` values on top.
    pub config: Config,
    /// The config as loaded from the file (and `-f`), before runtime options.
    pub base_config: Config,
    /// Config file given with `-f`; `reload-config` re-reads it instead of the default location.
    pub config_path: Option<PathBuf>,
    pub keymap: Keymap,
    pub mode: Mode,
    pub history: Vec<String>,
    pub message: Option<(String, Instant)>,
    pub repeat_until: Option<Instant>,
    /// When the prefix key was pressed; the shortcut popup waits `which_key_delay_ms` after it.
    pub prefix_at: Option<Instant>,
    /// Pane and cell of a left-button press; a drag from it starts a copy-mode selection.
    pub press: Option<(String, usize, usize)>,
    /// A split border held with the mouse: dragging moves it.
    pub border_drag: Option<BorderGrab>,
    /// Copy-mode state of panes that are not in the foreground: a pane keeps its scroll position,
    /// cursor and selection while the prefix is pending or focus moves elsewhere.
    pub parked_copy: HashMap<String, CopyState>,
    pub vt: HashMap<String, VtScreen>,
    pub passthroughs: Vec<Vec<u8>>,
    pub pane_dims: HashMap<String, (u16, u16)>,
    /// Byte offset each pane's screen has absorbed; the next output chunk must start there.
    pub expected: HashMap<String, u64>,
    pub layout: LayoutCache,
    /// Terminal width and height available to panes (status row excluded).
    pub viewport: (u16, u16),
    /// Panes drawn this frame (the zoomed pane alone when zoomed).
    pub panes: Vec<PaneRect>,
    pub active_pane: String,
    pub active_rect: Option<Rect>,
    pub marked_rect: Option<Rect>,
    pub focus_pane: Option<String>,
    pub zoomed: Option<String>,
    pub split_counter: usize,
    /// Nodes of the attached session, refreshed every tick.
    pub nodes: Vec<NodeInfo>,
    pub sub: Subscription,
    /// When the latest frames that showed new pane output were drawn, oldest first, at most
    /// `FRAMES_PER_BUDGET` of them.
    pub output_frames: VecDeque<Instant>,
    /// When the frame that shows pending pane output is drawn.
    pub frame_due: Option<Instant>,
    /// The last key only went to the active pane: nothing on screen changed until it answers.
    pub forwarded: bool,
    /// When the last key was pressed, and how many of its frames may still skip the budget.
    pub last_key: Option<Instant>,
    pub key_frames: u8,
    /// Directory holding the session sockets, used to discover and switch sessions.
    pub socket_dir: PathBuf,
    /// Fixed screen size (columns, rows) instead of the real terminal's.
    pub fixed_size: Option<(u16, u16)>,
    /// Where yanks go; `Captured` keeps them in memory instead of the system clipboard.
    pub clipboard: Clipboard,
    /// What the terminal currently shows, so frames send only changed cells.
    pub canvas: Canvas,
    pub last_copy: Option<String>,
    /// Text yanked while handling the current event, pushed to the session's paste buffers after it.
    pub yanks: Vec<String>,
    /// Default foreground and background of the host terminal, read at attach.
    pub host_colors: super::render::HostColors,
}

/// Everything the app reads from its surroundings, so it can run against explicit inputs.
#[derive(Debug, Clone)]
pub struct AppOptions {
    pub config: Config,
    /// Config file the config was loaded from (`-f`), re-read by `reload-config`.
    pub config_path: Option<PathBuf>,
    pub socket_dir: PathBuf,
    /// Screen size to lay out for; `None` follows the real terminal.
    pub size: Option<(u16, u16)>,
}

/// Where yanked text goes: the system clipboard, or a capture list that tests read back.
#[derive(Debug, Default)]
pub enum Clipboard {
    #[default]
    System,
    Captured(Vec<String>),
}

impl Clipboard {
    pub fn copy(&mut self, text: &str) {
        match self {
            Self::System => copy_to_clipboard(text),
            Self::Captured(texts) => texts.push(text.to_string()),
        }
    }
}

impl App {
    /// Attaches to the session: ensures a running window, subscribes to output and builds the app.
    pub async fn open(
        endpoint: &str,
        session: &str,
        _start: StartView,
        opts: AppOptions,
    ) -> Result<(Self, StreamRx), String> {
        let AppOptions {
            config,
            config_path,
            socket_dir,
            size,
        } = opts;
        let keymap = keymap::resolve(&config)?;
        let mut client = Client::connect(endpoint).await?;
        let sub_client = Client::connect(endpoint).await?;
        ensure_running_window(&mut client).await;
        let (tx, rx) = mpsc::channel(STREAM_QUEUE);
        let mut app = App {
            endpoint: endpoint.to_string(),
            session: session.to_string(),
            client,
            base_config: config.clone(),
            config,
            config_path,
            keymap,
            mode: Mode::Normal,
            history: Vec::new(),
            message: None,
            repeat_until: None,
            prefix_at: None,
            press: None,
            border_drag: None,
            parked_copy: HashMap::new(),
            vt: HashMap::new(),
            passthroughs: Vec::new(),
            pane_dims: HashMap::new(),
            expected: HashMap::new(),
            layout: LayoutCache::new(),
            viewport: (80, 23),
            panes: Vec::new(),
            active_pane: String::new(),
            active_rect: None,
            marked_rect: None,
            focus_pane: None,
            zoomed: None,
            split_counter: 1,
            nodes: Vec::new(),
            sub: Subscription {
                generation: 0,
                tx,
                task: None,
            },
            output_frames: VecDeque::new(),
            frame_due: None,
            forwarded: false,
            last_key: None,
            key_frames: 0,
            socket_dir,
            fixed_size: size,
            clipboard: Clipboard::System,
            canvas: Canvas::default(),
            last_copy: None,
            yanks: Vec::new(),
            host_colors: super::render::HostColors::default(),
        };
        app.subscribe(sub_client);
        app.apply_runtime().await;
        app.refresh_nodes().await;
        if let Ok(info) = app.client.info().await
            && let Some(restored_ms) = info.restored_ms
        {
            app.say(restored_notice(restored_ms, local_utc_offset(restored_ms)));
        }
        Ok((app, rx))
    }

    /// Replaces the event subscription with one bound to `client`'s session.
    pub fn subscribe(&mut self, client: Client) {
        if let Some(task) = self.sub.task.take() {
            task.abort();
        }
        self.sub.generation += 1;
        let generation = self.sub.generation;
        let tx = self.sub.tx.clone();
        self.sub.task = Some(tokio::spawn(async move {
            let _ = client
                .stream_logs_into((None, None), true, tx, move |event| (generation, event))
                .await;
        }));
    }

    /// Fetches the layout and pane sizes for the next frame. Returns false when nothing is left to show.
    pub async fn prepare_frame(&mut self) -> bool {
        let (w, h) = self
            .fixed_size
            .unwrap_or_else(|| crossterm::terminal::size().unwrap_or((80, 24)));
        let layout_h = h.saturating_sub(1);
        self.viewport = (w, layout_h);
        if !self.layout.sync(&mut self.client, 0, w, layout_h).await {
            return false;
        }
        let focused = self
            .layout
            .raw
            .iter()
            .find(|(.., focused)| *focused)
            .map(|(name, ..)| name.clone())
            .unwrap_or_default();
        self.follow_focus(focused);
        let shown_before: Vec<String> = self.panes.iter().map(|(name, ..)| name.clone()).collect();
        self.panes = match &self.zoomed {
            Some(name) => vec![(name.clone(), Rect::new(0, 0, w, layout_h), true)],
            None => self.layout.raw.clone(),
        };
        self.active_rect = self.panes.iter().find(|(.., f)| *f).map(|(_, r, _)| *r);
        let marked_name = self.layout.marked_pane();
        self.marked_rect = marked_name.and_then(|m| {
            self.panes.iter().find(|(name, ..)| name == m).map(|(_, r, _)| *r)
        });
        self.refresh_newly_shown(&shown_before).await;
        if self.focus_pane.as_deref() != Some(self.active_pane.as_str()) {
            self.recheck_size(&self.active_pane.clone()).await;
        }
        // While a border is dragged, panes are drawn at their new places as they are: resizing
        // their terminals and rebuilding their screens waits for the release.
        if self.border_drag.is_none() {
            self.sync_pane_sizes().await;
        }
        self.route_focus().await;
        true
    }

    async fn sync_pane_sizes(&mut self) {
        sync_dims(
            &mut self.client,
            &self.panes,
            &mut self.pane_dims,
            &mut self.vt,
            &mut self.expected,
        )
        .await;
    }

    /// A pane coming into focus gets its size checked: if something else resized its terminal,
    /// the next sync sends the right size again and the program reflows.
    /// Only on focus: two clients of different sizes attached to one session would otherwise
    /// take a pane back and forth.
    async fn recheck_size(&mut self, name: &str) {
        if let Ok(info) = self.client.status(name).await {
            for name in resized_elsewhere(&self.pane_dims, &[info]) {
                self.pane_dims.remove(&name);
            }
        }
    }

    /// Sends focus-out to the previously active pane and focus-in to the new one, where enabled.
    async fn route_focus(&mut self) {
        if self.active_pane.is_empty()
            || self.focus_pane.as_deref() == Some(self.active_pane.as_str())
        {
            return;
        }
        if let Some(old) = self.focus_pane.take() {
            self.send_focus(&old, false).await;
        }
        let active = self.active_pane.clone();
        self.send_focus(&active, true).await;
        self.focus_pane = Some(active);
    }

    async fn send_focus(&mut self, name: &str, focused: bool) {
        if self.modes_of(name).focus_events {
            let text = String::from_utf8_lossy(crate::keys::focus_event(focused)).into_owned();
            let _ = self.client.send_input(name, &text).await;
        }
    }

    /// Width and height of the area the panes are laid out in, right of the sidebar.
    pub fn pane_viewport(&self) -> (u16, u16) {
        self.viewport
    }

    pub fn modes_of(&self, name: &str) -> TermModes {
        self.vt.get(name).map(|vt| vt.modes()).unwrap_or_default()
    }

    /// Makes `focused` the active pane. Copy mode belongs to a pane: leaving the pane parks it and
    /// arriving at a pane with parked copy state resumes it.
    fn follow_focus(&mut self, focused: String) {
        if focused != self.active_pane {
            self.park_copy();
        }
        self.active_pane = focused;
        if matches!(self.mode, Mode::Normal)
            && let Some(cs) = self.parked_copy.remove(&self.active_pane)
        {
            self.mode = Mode::Copy(cs);
        }
        let live: std::collections::HashSet<&str> =
            self.layout.raw.iter().map(|(n, ..)| n.as_str()).collect();
        self.parked_copy
            .retain(|name, _| live.contains(name.as_str()));
    }

    /// Moves an active copy mode out of the foreground, keyed by the active pane.
    pub fn park_copy(&mut self) {
        if matches!(self.mode, Mode::Copy(_))
            && let Mode::Copy(cs) = std::mem::replace(&mut self.mode, Mode::Normal)
        {
            self.parked_copy.insert(self.active_pane.clone(), cs);
        }
    }

    pub fn active_vt(&self) -> Option<&VtScreen> {
        self.vt.get(&self.active_pane)
    }

    pub async fn refresh_nodes(&mut self) {
        if let Ok(nodes) = self.client.list().await {
            self.nodes = nodes;
        }
    }

    /// Applies stream events from the current subscription. The session ending is noticed when the
    /// next frame finds no panes in the layout.
    pub async fn on_stream(
        &mut self,
        first: Option<(u64, StreamEvent)>,
        rx: &mut StreamRx,
    ) -> Flow {
        let Some(first) = first else {
            return Flow::Continue;
        };
        let mut batch = vec![first];
        drain_into(rx, &mut batch);
        // The rest of a burst (a pager page arrives in several reads) is often already on its way:
        // yielding lets it land in this frame instead of a later one, at no timer cost.
        for _ in 0..BURST_YIELDS {
            tokio::task::yield_now().await;
            if !drain_into(rx, &mut batch) {
                break;
            }
        }
        let echo = batch.iter().any(|(_, event)| {
            matches!(event, StreamEvent::Output { name, .. } if *name == self.active_pane)
        });
        // Output of panes nobody sees changes nothing on screen: no frame for it.
        let shown = batch.iter().any(|(_, event)| match event {
            StreamEvent::Output { name, .. } => self.keeps_screen(name),
            _ => true,
        });
        if matches!(self.apply_batch(batch).await, Flow::Quit) {
            return Flow::Quit;
        }
        if !shown {
            return Flow::Continue;
        }
        let now = Instant::now();
        // The focused pane's output right after a keypress is likely its response: it goes out at
        // once, whatever other panes are flooding, for a few frames per keypress.
        let typing = echo
            && self.key_frames > 0
            && self
                .last_key
                .is_some_and(|at| now.saturating_duration_since(at) < TYPING_WINDOW);
        if typing {
            self.key_frames -= 1;
        }
        let window_start = (self.output_frames.len() == FRAMES_PER_BUDGET)
            .then(|| self.output_frames.front().copied())
            .flatten();
        self.frame_due = Some(if typing {
            now
        } else {
            next_frame(window_start, now, self.frame_due)
        });
        Flow::Continue
    }

    /// Records a drawn frame: it shows all output received so far. Only a frame that showed new
    /// output counts against the frame budget; a redraw for a keypress must not delay the output
    /// that keypress provokes.
    pub fn drew(&mut self, at: Instant) {
        if self.frame_due.take().is_some() {
            self.output_frames.push_back(at);
            if self.output_frames.len() > FRAMES_PER_BUDGET {
                self.output_frames.pop_front();
            }
        }
    }

    pub async fn on_tick(&mut self) -> Flow {
        self.layout.dirty = true;
        self.refresh_nodes().await;
        if matches!(self.mode, Mode::Overlay(_)) {
            self.refresh_overlay().await;
        }
        Flow::Continue
    }

    /// Applies the events of the current subscription; a client command may end the session view.
    async fn apply_batch(&mut self, batch: Vec<(u64, StreamEvent)>) -> Flow {
        for (generation, event) in batch {
            if generation != self.sub.generation {
                continue;
            }
            if let StreamEvent::ClientCommand { line } = event {
                if matches!(self.run_line(&line).await, Flow::Quit) {
                    return Flow::Quit;
                }
            } else if self.apply_event(event).await {
                self.layout.dirty = true;
            }
        }
        Flow::Continue
    }

    pub async fn on_event(&mut self, ev: Event) -> Flow {
        let flow = self.handle_event(ev).await;
        for text in std::mem::take(&mut self.yanks) {
            let _ = self.client.set_buffer(None, &text).await;
        }
        flow
    }

    async fn handle_event(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.last_key = Some(Instant::now());
                self.key_frames = TYPING_FRAMES;
                self.on_key(key).await
            }
            Event::Mouse(mouse) => self.on_mouse(mouse).await,
            Event::Paste(text) => self.on_paste(&text).await,
            Event::FocusGained => self.on_focus(true).await,
            Event::FocusLost => self.on_focus(false).await,
            Event::Resize(..) => {
                self.layout.dirty = true;
                self.canvas.invalidate();
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    /// Records a message for the status row; it expires after a few seconds.
    pub fn say(&mut self, text: impl Into<String>) -> Flow {
        self.message = Some((text.into(), Instant::now()));
        Flow::Continue
    }

    /// The live message, if one was set within the last few seconds.
    pub fn live_message(&self) -> Option<&str> {
        self.message
            .as_ref()
            .filter(|(_, at)| at.elapsed() < MESSAGE_TTL)
            .map(|(text, _)| text.as_str())
    }

    pub fn remember(&mut self, line: &str) {
        if self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_string());
            if self.history.len() > HISTORY_CAP {
                self.history.remove(0);
            }
        }
    }

    pub fn repeating(&self) -> bool {
        self.repeat_until
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn arm_repeat(&mut self) {
        self.repeat_until = Some(Instant::now() + REPEAT_WINDOW);
    }

    /// Enters prefix mode; the shortcut popup follows after the configured delay.
    pub fn enter_prefix(&mut self) {
        self.mode = Mode::Prefix;
        self.prefix_at = Some(Instant::now());
    }

    /// When the pending shortcut popup is due, while prefix mode waits for its next key.
    pub fn hud_deadline(&self) -> Option<Instant> {
        let delay = Duration::from_millis(self.config.which_key_delay_ms);
        match self.mode {
            Mode::Prefix => self.prefix_at.map(|at| at + delay),
            _ => None,
        }
    }

    /// Whether the shortcut popup shows: prefix mode has lasted at least the configured delay.
    pub fn hud_visible(&self) -> bool {
        self.hud_deadline().is_some_and(|due| Instant::now() >= due)
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Flow {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key).await,
            Mode::Prefix => self.prefix_key(key).await,
            Mode::Copy(cs) => self.copy_key(cs, key).await,
            Mode::Overlay(ov) => self.overlay_key(ov, key).await,
            Mode::Prompt(p) => self.prompt_key(p, key).await,
            Mode::Confirm(prompt, cmd) => self.confirm_key(prompt, cmd, key).await,
        }
    }
}

/// Creates a shell window when the session has no running pane.
async fn ensure_running_window(client: &mut Client) {
    let has_running = client
        .list()
        .await
        .map(|nodes| nodes.iter().any(|n| n.is_running()))
        .unwrap_or(false);
    if has_running {
        return;
    }
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let spawn = SpawnOpts {
        command: default_shell(),
        pty: true,
        cwd,
        ..SpawnOpts::default()
    };
    let _ = client
        .new_window(NewWindowOpts {
            name: None,
            spawn,
            focus: true,
        })
        .await;
}

/// When to draw output that arrived at `now`: a frame already scheduled stands; otherwise at once,
/// unless the budget is spent: `window_start` is the oldest of the last `FRAMES_PER_BUDGET` output
/// frames when that many were drawn. No settle delay: events that arrived together are already one
/// batch, and timers are too coarse (whole milliseconds) to wait for less than that.
pub fn next_frame(window_start: Option<Instant>, now: Instant, pending: Option<Instant>) -> Instant {
    if let Some(due) = pending {
        return due;
    }
    match window_start {
        Some(oldest) => now.max(oldest + FRAME_BUDGET),
        None => now,
    }
}

/// The last of a run of mouse drags already queued after `first`, and the first queued event that
/// is not part of the run, to handle next. A fast drag sends many motions; only the latest
/// position matters.
pub fn latest_drag(first: Event, mut queued: impl FnMut() -> Option<Event>) -> (Event, Option<Event>) {
    let is_drag = |ev: &Event| matches!(ev, Event::Mouse(m) if matches!(m.kind, MouseEventKind::Drag(_)));
    if !is_drag(&first) {
        return (first, None);
    }
    let mut last = first;
    while let Some(next) = queued() {
        if !is_drag(&next) {
            return (last, Some(next));
        }
        last = next;
    }
    (last, None)
}

/// Moves every event already queued into `batch`; returns whether there was any.
fn drain_into(rx: &mut StreamRx, batch: &mut Vec<(u64, StreamEvent)>) -> bool {
    let before = batch.len();
    while let Ok(more) = rx.try_recv() {
        batch.push(more);
    }
    batch.len() > before
}

/// Whether to draw after a wake-up. Pane output waits for its frame unless that frame is already
/// due (drawn now rather than through a timer, which would add up to a millisecond); a key that
/// only went to a pane changes nothing until the pane answers; anything else shows at once.
pub fn needs_redraw(output: bool, forwarded: bool, frame_due: Option<Instant>, now: Instant) -> bool {
    let due = frame_due.is_some_and(|at| at <= now);
    due || !(output || forwarded)
}

/// The earliest of the given deadlines.
fn earliest(deadlines: impl IntoIterator<Item = Option<Instant>>) -> Option<Instant> {
    deadlines.into_iter().flatten().min()
}

/// One wake-up of the event loop; handlers run after the select has released its borrows.
enum Wake {
    Input(Event),
    Stream(Option<(u64, StreamEvent)>),
    Tick,
    /// A delayed element is due: pane output's frame, the shortcut popup, or a sidebar switch.
    Due,
}

/// Terminal input read on its own thread and handed over a channel. crossterm's async
/// `EventStream` takes an internal lock on every poll while its reader thread holds it, so the
/// event loop spun on that lock each time pane output woke it.
/// Terminal input, with a run of queued mouse drags merged into its last position.
struct Input {
    rx: UnboundedReceiver<Event>,
    /// The event that ended the last merged run, handed out next.
    held: Option<Event>,
}

impl Input {
    /// Takes the held event, if any, without waiting: it then completes in the same poll, so
    /// `select!` never drops it.
    async fn recv(&mut self) -> Option<Event> {
        let first = match self.held.take() {
            Some(event) => event,
            None => self.rx.recv().await?,
        };
        let (event, next) = latest_drag(first, || self.rx.try_recv().ok());
        self.held = next;
        Some(event)
    }
}

fn terminal_input() -> UnboundedReceiver<Event> {
    let (tx, rx) = unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    rx
}

/// Runs the attached TUI until detach, session end or no panes left.
pub async fn run_tui(
    endpoint: &str,
    session: &str,
    start: StartView,
    opts: AppOptions,
) -> Result<(), String> {
    let _guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {e}"))?;
    let report = super::terminal::query_terminal();
    let (mut app, mut stream) = App::open(endpoint, session, start, opts).await?;
    app.canvas.set_lr_margins(report.margins);
    app.host_colors = super::render::HostColors::from_report(&report);
    let mut keys = Input {
        rx: terminal_input(),
        held: None,
    };
    let mut out = BufWriter::with_capacity(64 * 1024, stdout());
    let mut body = Vec::with_capacity(64 * 1024);
    let (mut cursor, mut last_cursor) = (Vec::new(), Vec::new());
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut redraw = true;
    loop {
        if redraw {
            if !app.prepare_frame().await {
                return Ok(());
            }
            body.clear();
            cursor.clear();
            let _ = frame::draw_body(&mut app, &mut body);
            let _ = frame::place_cursor(&app, &mut cursor);
            // A frame with no changes that leaves the cursor where it is would only be bytes for
            // the terminal to parse.
            if !body.is_empty() || cursor != last_cursor {
                let _ = frame::send(&mut out, &body, &cursor);
                std::mem::swap(&mut cursor, &mut last_cursor);
            }
            app.drew(Instant::now());
        }
        let popup = app.hud_deadline().filter(|_| !app.hud_visible());
        let due = earliest([popup, app.frame_due]);
        let wake = tokio::select! {
            Some(event) = keys.recv() => Wake::Input(event),
            item = stream.recv() => Wake::Stream(item),
            _ = tick.tick() => Wake::Tick,
            _ = sleep_until_opt(due.map(tokio::time::Instant::from_std)) => Wake::Due,
        };
        let output = matches!(wake, Wake::Stream(_));
        let flow = match wake {
            Wake::Input(event) => app.on_event(event).await,
            Wake::Stream(item) => {
                let flow = app.on_stream(item, &mut stream).await;
                flush_passthroughs(&mut out, &mut app.passthroughs);
                flow
            }
            Wake::Tick => app.on_tick().await,
            Wake::Due => Flow::Continue,
        };
        let forwarded = std::mem::take(&mut app.forwarded);
        redraw = needs_redraw(output, forwarded, app.frame_due, Instant::now());
        if flow == Flow::Quit {
            return Ok(());
        }
    }
}

fn flush_passthroughs<W: Write>(out: &mut W, passthroughs: &mut Vec<Vec<u8>>) {
    if passthroughs.is_empty() {
        return;
    }
    for p in passthroughs.drain(..) {
        let _ = out.write_all(&p);
    }
    let _ = out.flush();
}
