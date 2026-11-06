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
use super::frame;
use super::job::Job;
use super::keymap::{self, Keymap};
use super::menu::MenuState;
use super::outer::OuterTerminal;
use super::overlay::Overlay;
use super::panes::{BorderGrab, LayoutCache, PaneRect, resized_elsewhere, sync_dims};
use super::popup::{PopupEvent, PopupState};
use super::prompt::PromptRun;
use super::render::copy_to_clipboard;
use super::status::{local_utc_offset, restored_notice, time_of_day};
use super::statusline::{self, Rows, Target};
use super::terminal::TerminalGuard;
use crate::client::Client;
use crate::config::{Config, ExtendedKeys, StatusPosition};
use crate::keys::TermModes;
use crate::layout::geometry::Rect;
use crate::node::sleep_until_opt;
use crate::protocol::{ClientInfo, NewWindowOpts, NodeInfo, SpawnOpts, StreamEvent};
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
    Prompt(PromptRun),
    Confirm(String, Command),
    /// display-panes: pane numbers on screen until `until` (`None`: until a key is pressed).
    Panes {
        until: Option<Instant>,
    },
    /// clock-mode: a large clock fills `pane` until a key is pressed.
    Clock {
        pane: String,
    },
    /// display-popup: a command runs in a box over the panes; its keys go to it until it closes.
    Popup(Box<PopupState>),
    /// display-menu: a menu of commands over the panes.
    Menu(Box<MenuState>),
}

/// Event-stream subscription for the attached session. Events tagged with an older
/// generation come from a previous session and are dropped.
pub struct Subscription {
    pub generation: u64,
    pub tx: mpsc::Sender<(u64, StreamEvent)>,
    pub task: Option<JoinHandle<()>>,
}

impl Subscription {
    fn new(tx: mpsc::Sender<(u64, StreamEvent)>) -> Self {
        Self {
            generation: 0,
            tx,
            task: None,
        }
    }
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
const HISTORY_CAP: usize = 100;
/// Status-row messages kept for `show-messages`.
const MESSAGE_LOG_CAP: usize = 100;

pub struct App {
    pub endpoint: String,
    pub session: String,
    pub client: Client,
    pub config: Config,
    pub keymap: Keymap,
    pub mode: Mode,
    /// Copy-mode state of panes that are not in the foreground: a pane keeps its scroll position,
    /// cursor and selection while the prefix is pending or focus moves elsewhere.
    pub parked_copy: HashMap<String, CopyState>,
    /// Panes that just left copy mode, whose screens still miss the output they froze through.
    pub released: Vec<String>,
    pub vt: HashMap<String, VtScreen>,
    pub passthroughs: Vec<Vec<u8>>,
    pub pane_dims: HashMap<String, (u16, u16)>,
    /// Byte offset each pane's screen has absorbed; the next output chunk must start there.
    pub expected: HashMap<String, u64>,
    pub layout: LayoutCache,
    /// Terminal width and height available to panes (status row excluded).
    pub viewport: (u16, u16),
    /// Rows on screen: the pane area, the status line and the message row.
    pub rows: Rows,
    /// Panes drawn this frame (the zoomed pane alone when zoomed).
    pub panes: Vec<PaneRect>,
    pub active_pane: String,
    pub active_rect: Option<Rect>,
    pub marked_rect: Option<Rect>,
    pub focus_pane: Option<String>,
    pub split_counter: usize,
    /// Nodes of the attached session, refreshed every tick.
    pub nodes: Vec<NodeInfo>,
    pub sub: Subscription,
    /// What the terminal currently shows, so frames send only changed cells.
    pub canvas: Canvas,
    /// Commands that wait on the daemon report back here (see `job`).
    pub jobs: mpsc::UnboundedSender<Job>,
    pub job_rx: UnboundedReceiver<Job>,
    /// What this client shows on the outer terminal beyond the panes: title, bells, activity.
    pub outer: OuterTerminal,
    /// Frame pacing: when pane output is drawn and when a keypress lets its echo skip the budget.
    pub pacing: FramePacing,
    /// Input in progress: prompt history, the status-row message, key repeat, prefix and mouse presses.
    pub interaction: Interaction,
    /// Where this client was launched: its config file, socket directory and any fixed size.
    pub launch: Launch,
    /// The status line as last drawn: its clickable items and the columns its zones end at.
    pub status_hits: StatusHits,
    /// Yanked text and where yanks go.
    pub yank: YankState,
    /// The host terminal's colors, as reported at attach and as panes' color sequences changed them.
    pub host: HostTerm,
    /// What the app shows beyond the panes: the message log, clock mode's time source and suspension.
    pub chrome: Chrome,
    /// This TUI as its session's client, and the session it is attached to.
    pub attachment: Attachment,
}

/// Frame pacing state (see `drew` and `prepare_frame`).
#[derive(Default)]
pub struct FramePacing {
    /// When the last frame was drawn; paces `status-interval`.
    pub drawn_at: Option<Instant>,
    /// When the latest frames that showed new pane output were drawn, oldest first, at most
    /// `FRAMES_PER_BUDGET` of them.
    pub output_frames: VecDeque<Instant>,
    /// When the frame that shows pending pane output is drawn.
    pub frame_due: Option<Instant>,
    /// The last key only went to the active pane: nothing on screen changed until it answers.
    pub forwarded: bool,
    /// When the last key was pressed.
    pub last_key: Option<Instant>,
    /// How many of the last key's frames may still skip the budget.
    pub key_frames: u8,
}

/// Input in progress: the command history, the status-row message, key repeat, the prefix and the
/// mouse presses.
#[derive(Default)]
pub struct Interaction {
    /// Command-prompt lines remembered, oldest first (capped at `HISTORY_CAP`).
    pub history: Vec<String>,
    /// The status-row message and when it was set.
    pub message: Option<(String, Instant)>,
    /// When the window in which a repeated key still counts as repeated ends.
    pub repeat_until: Option<Instant>,
    /// When the prefix key was pressed; the shortcut popup waits `which_key_delay_ms` after it.
    pub prefix_at: Option<Instant>,
    /// Pane and cell of a left-button press; a drag from it starts a copy-mode selection.
    pub press: Option<(String, usize, usize)>,
    /// The current left-button drag started copy mode: releasing it copies and leaves, so the
    /// pane streams again.
    pub drag_started_copy: bool,
    /// A split border held with the mouse: dragging moves it.
    pub border_drag: Option<BorderGrab>,
}

/// Where this client was launched from.
pub struct Launch {
    /// Config file given with `-f`; `reload-config` re-reads it instead of the default location.
    pub config_path: Option<PathBuf>,
    /// Directory holding the session sockets, used to discover and switch sessions.
    pub socket_dir: PathBuf,
    /// Fixed screen size (columns, rows) instead of the real terminal's.
    pub fixed_size: Option<(u16, u16)>,
}

impl Launch {
    fn new(
        config_path: Option<PathBuf>,
        socket_dir: PathBuf,
        fixed_size: Option<(u16, u16)>,
    ) -> Self {
        Self {
            config_path,
            socket_dir,
            fixed_size,
        }
    }
}

/// The status line as last drawn.
pub struct StatusHits {
    /// Clickable items as `[start, end)` columns and their target.
    pub targets: Vec<(usize, usize, Target)>,
    /// Columns the status line's `status-left` ends before and `status-right` starts at.
    pub zones: (usize, usize),
}

impl Default for StatusHits {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            zones: (0, usize::MAX),
        }
    }
}

/// Yanked text and where it goes.
pub struct YankState {
    /// Where yanks go; `Captured` keeps them in memory instead of the system clipboard.
    pub clipboard: Clipboard,
    /// The text of the last yank.
    pub last_copy: Option<String>,
    /// Text yanked while handling the current event, pushed to the session's paste buffers after it.
    pub yanks: Vec<String>,
}

impl YankState {
    fn new(clipboard: Clipboard) -> Self {
        Self {
            clipboard,
            last_copy: None,
            yanks: Vec::new(),
        }
    }
}

/// The host terminal's colors.
#[derive(Default)]
pub struct HostTerm {
    /// Default foreground and background of the host terminal, read at attach.
    pub colors: super::render::HostColors,
    /// What the terminal reported at attach, which a color reset goes back to.
    pub original: super::render::HostColors,
}

/// What the app shows beyond the panes.
#[derive(Default)]
pub struct Chrome {
    /// The last `MESSAGE_LOG_CAP` status-row messages, oldest first (`show-messages`).
    pub message_log: Vec<String>,
    /// Set by `suspend-client`; the event loop stops the process once the command has run.
    pub suspend_requested: bool,
    /// Where clock mode reads the time.
    pub clock: ClockSource,
}

/// This client as its session's client.
pub struct Attachment {
    /// This TUI as its session's client: its name, the size it reports and its read-only flag.
    pub terminal: ClientInfo,
    /// Clients attached to this session, counted every tick (`session_attached`).
    pub attached: usize,
    /// When the session's daemon started and whether it restored a snapshot, for the time variables.
    pub times: crate::format::SessionTimes,
    /// The session this client was attached to before the current one (`switch-client -l`).
    pub previous_session: Option<String>,
}

impl Attachment {
    fn new(terminal: ClientInfo) -> Self {
        Self {
            terminal,
            attached: 0,
            times: crate::format::SessionTimes::default(),
            previous_session: None,
        }
    }
}

/// Where clock mode reads the time: the system clock, or a fixed time (for tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockSource {
    #[default]
    System,
    Fixed(u8, u8),
}

impl ClockSource {
    /// Local hour and minute now, 24-hour.
    pub fn hour_minute(self) -> (u8, u8) {
        match self {
            Self::Fixed(hour, minute) => (hour, minute),
            Self::System => {
                let now = crate::node::now_ms();
                let (hour, minute, _) = time_of_day(now, local_utc_offset(now));
                (hour as u8, minute as u8)
            }
        }
    }
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
    /// How this client attaches (`attach -d`, `attach -r`).
    pub attach: AttachOpts,
    /// The name this client is listed under; the terminal's device path when `None`.
    pub client_name: Option<String>,
}

/// How a client attaches: `exclusive` detaches the session's other clients (`attach -d`), `hangup`
/// (with `exclusive`) also hangs up their parents (`attach -x`), and `read_only` keeps every key and
/// paste from reaching the panes (`attach -r`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachOpts {
    pub exclusive: bool,
    pub hangup: bool,
    pub read_only: bool,
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
        let keymap = keymap::resolve(&opts.config)?;
        let exclusive = opts.attach.exclusive;
        let hangup = opts.attach.hangup;
        let mut client = Client::connect(endpoint).await?;
        let sub_client = Client::connect(endpoint).await?;
        let started = ensure_running_window(&mut client).await;
        let (tx, rx) = mpsc::channel(STREAM_QUEUE);
        let mut app = Self::new(endpoint, session, client, keymap, tx, opts);
        app.report("new-window", started);
        app.subscribe_as(sub_client, exclusive, hangup);
        app.refresh_attached().await;
        app.apply_bindings().await;
        app.refresh_nodes().await;
        if let Ok(info) = app.client.info().await {
            // The daemon's name wins over the socket's: `rename-session` changes one, not the other.
            app.attachment.times = crate::format::SessionTimes {
                started_ms: info.created_ms,
                restored_ms: info.restored_ms,
            };
            app.session = info.name;
            if let Some(restored_ms) = info.restored_ms {
                app.say(restored_notice(restored_ms, local_utc_offset(restored_ms)));
            }
        }
        Ok((app, rx))
    }

    /// The app before it is attached: its state from the given parts and options.
    fn new(
        endpoint: &str,
        session: &str,
        client: Client,
        keymap: Keymap,
        tx: mpsc::Sender<(u64, StreamEvent)>,
        opts: AppOptions,
    ) -> Self {
        let AppOptions {
            config,
            config_path,
            socket_dir,
            size,
            attach,
            client_name,
        } = opts;
        let (jobs, job_rx) = unbounded_channel();
        let terminal = terminal_info(session, client_name, attach.read_only);
        App {
            endpoint: endpoint.to_string(),
            session: session.to_string(),
            client,
            config,
            keymap,
            mode: Mode::Normal,
            parked_copy: HashMap::new(),
            released: Vec::new(),
            vt: HashMap::new(),
            passthroughs: Vec::new(),
            pane_dims: HashMap::new(),
            expected: HashMap::new(),
            layout: LayoutCache::new(),
            viewport: (80, 23),
            rows: statusline::rows(24, 1, false),
            panes: Vec::new(),
            active_pane: String::new(),
            active_rect: None,
            marked_rect: None,
            focus_pane: None,
            split_counter: 1,
            nodes: Vec::new(),
            sub: Subscription::new(tx),
            canvas: Canvas::default(),
            jobs,
            job_rx,
            outer: OuterTerminal::default(),
            pacing: FramePacing::default(),
            interaction: Interaction::default(),
            launch: Launch::new(config_path, socket_dir, size),
            status_hits: StatusHits::default(),
            yank: YankState::new(Clipboard::System),
            host: HostTerm::default(),
            chrome: Chrome::default(),
            attachment: Attachment::new(terminal),
        }
    }

    /// Replaces the event subscription with one bound to `client`'s session, as this client.
    pub fn subscribe(&mut self, client: Client) {
        self.subscribe_as(client, false, false);
    }

    /// Replaces the event subscription with one bound to `client`'s session, registered as this
    /// client (with `exclusive`, the session's other clients are detached first, and with `hangup`
    /// their parents are hung up).
    fn subscribe_as(&mut self, client: Client, exclusive: bool, hangup: bool) {
        if let Some(task) = self.sub.task.take() {
            task.abort();
        }
        self.sub.generation += 1;
        let generation = self.sub.generation;
        let tx = self.sub.tx.clone();
        let terminal = self.attachment.terminal.clone();
        self.sub.task = Some(tokio::spawn(async move {
            let _ = client
                .stream_as_client(terminal, exclusive, hangup, tx, move |event| {
                    (generation, event)
                })
                .await;
        }));
    }

    /// Fetches the layout and pane sizes for the next frame. Returns false when nothing is left to show.
    pub async fn prepare_frame(&mut self) -> bool {
        let (w, h) = self
            .launch
            .fixed_size
            .unwrap_or_else(|| crossterm::terminal::size().unwrap_or((80, 24)));
        let status_lines = if self.config.status {
            self.config.status_lines
        } else {
            0
        };
        let rows = statusline::rows(
            h,
            status_lines,
            self.config.status_position == StatusPosition::Top,
        );
        if rows != self.rows {
            self.rows = rows;
            self.canvas.invalidate();
        }
        self.viewport = (w, rows.pane_h);
        self.report_size(w, rows.pane_h).await;
        if !self
            .layout
            .sync(&mut self.client, (0, rows.pane_top), (w, rows.pane_h))
            .await
        {
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
        self.refresh_outer((w, h));
        let shown_before: Vec<String> = self.panes.iter().map(|(name, ..)| name.clone()).collect();
        self.panes = if self.zoomed() {
            // Zoomed: the focused pane alone fills the pane area.
            self.layout
                .raw
                .iter()
                .filter(|(.., focused)| *focused)
                .map(|(name, ..)| {
                    (
                        name.clone(),
                        Rect::new(0, rows.pane_top, w, rows.pane_h),
                        true,
                    )
                })
                .collect()
        } else {
            self.layout.raw.clone()
        };
        self.active_rect = self.panes.iter().find(|(.., f)| *f).map(|(_, r, _)| *r);
        let marked_name = self.layout.marked_pane();
        self.marked_rect = marked_name.and_then(|m| {
            self.panes
                .iter()
                .find(|(name, ..)| name == m)
                .map(|(_, r, _)| *r)
        });
        self.catch_up_released().await;
        self.refresh_newly_shown(&shown_before).await;
        if self.focus_pane.as_deref() != Some(self.active_pane.as_str()) {
            self.recheck_size(&self.active_pane.clone()).await;
        }
        // While a border is dragged, panes are drawn at their new places as they are: resizing
        // their terminals and rebuilding their screens waits for the release.
        if self.interaction.border_drag.is_none() {
            self.sync_pane_sizes().await;
        }
        self.route_focus().await;
        true
    }

    async fn sync_pane_sizes(&mut self) {
        let before: HashMap<String, (usize, usize)> = self
            .vt
            .iter()
            .map(|(name, vt)| (name.clone(), (vt.grid().history_len(), vt.grid().cols)))
            .collect();
        sync_dims(
            &mut self.client,
            &self.panes,
            &mut self.pane_dims,
            &mut self.vt,
            &mut self.expected,
        )
        .await;
        self.keep_copy_positions(&before);
    }

    /// Copy-mode positions of panes resized by `sync_pane_sizes` follow their lines through the
    /// rebuilt screens; `before` holds each pane's history length and width before the resize.
    fn keep_copy_positions(&mut self, before: &HashMap<String, (usize, usize)>) {
        for (name, &(old_hist, old_cols)) in before {
            let Some(grid) = self.vt.get(name).map(VtScreen::grid) else {
                continue;
            };
            let reflowed = grid.cols != old_cols;
            let copy = match &mut self.mode {
                Mode::Copy(cs) if *name == self.active_pane => Some(cs),
                _ => self.parked_copy.get_mut(name),
            };
            if let Some(cs) = copy {
                cs.resized(old_hist, grid, reflowed);
            }
        }
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

    /// Reports focus in or out to pane `name`, when the pane asked for reports.
    pub(super) async fn send_focus(&mut self, name: &str, focused: bool) {
        if name.is_empty()
            || !self.modes_of(name).focus_events
            || self.attachment.terminal.readonly
        {
            return;
        }
        let text = String::from_utf8_lossy(crate::keys::focus_event(focused)).into_owned();
        let _ = self.client.send_input(name, &text).await;
    }

    /// Width and height of the area the panes are laid out in, right of the sidebar.
    pub fn pane_viewport(&self) -> (u16, u16) {
        self.viewport
    }

    /// Whether the active window is zoomed (the daemon's flag, as the layout reports it).
    pub fn zoomed(&self) -> bool {
        self.layout.tabs.iter().any(|t| t.is_active && t.zoomed)
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
        let (runs, detached) = take_control_events(&mut batch);
        if detached {
            return Flow::Quit;
        }
        let echo = batch.iter().any(|(_, event)| {
            matches!(event, StreamEvent::Output { name, .. } if *name == self.active_pane)
        });
        // Output of panes nobody sees changes nothing on screen: no frame for it.
        let shown = batch.iter().any(|(_, event)| match event {
            StreamEvent::Output { name, .. } => self.keeps_screen(name),
            _ => true,
        });
        self.apply_batch(batch).await;
        for line in &runs {
            if self.run_forwarded(line).await == Flow::Quit {
                return Flow::Quit;
            }
        }
        if !shown && runs.is_empty() {
            return Flow::Continue;
        }
        let now = Instant::now();
        // The focused pane's output right after a keypress is likely its response: it goes out at
        // once, whatever other panes are flooding, for a few frames per keypress.
        let typing = echo
            && self.pacing.key_frames > 0
            && self
                .pacing
                .last_key
                .is_some_and(|at| now.saturating_duration_since(at) < TYPING_WINDOW);
        if typing {
            self.pacing.key_frames -= 1;
        }
        let window_start = (self.pacing.output_frames.len() == FRAMES_PER_BUDGET)
            .then(|| self.pacing.output_frames.front().copied())
            .flatten();
        self.pacing.frame_due = Some(if typing {
            now
        } else {
            next_frame(window_start, now, self.pacing.frame_due)
        });
        Flow::Continue
    }

    /// Records a drawn frame: it shows all output received so far. Only a frame that showed new
    /// output counts against the frame budget; a redraw for a keypress must not delay the output
    /// that keypress provokes.
    pub fn drew(&mut self, at: Instant) {
        self.pacing.drawn_at = Some(at);
        if self.pacing.frame_due.take().is_some() {
            self.pacing.output_frames.push_back(at);
            if self.pacing.output_frames.len() > FRAMES_PER_BUDGET {
                self.pacing.output_frames.pop_front();
            }
        }
    }

    pub async fn on_tick(&mut self) -> Flow {
        self.layout.dirty = true;
        self.refresh_nodes().await;
        self.refresh_attached().await;
        if matches!(self.mode, Mode::Overlay(_)) {
            self.refresh_overlay().await;
        }
        Flow::Continue
    }

    /// Applies the events of the current subscription.
    async fn apply_batch(&mut self, batch: Vec<(u64, StreamEvent)>) {
        for (generation, event) in batch {
            if generation != self.sub.generation {
                continue;
            }
            if self.apply_event(event).await {
                self.layout.dirty = true;
            }
        }
        self.after_input().await;
    }

    pub async fn on_event(&mut self, ev: Event) -> Flow {
        if matches!(ev, Event::Key(_) | Event::Mouse(_) | Event::Paste(_)) {
            self.touch_client().await;
        }
        let flow = self.handle_event(ev).await;
        self.after_input().await;
        flow
    }

    /// Work that follows every input or stream batch: copies made reach the paste buffers.
    async fn after_input(&mut self) {
        self.ring_bells();
        for text in std::mem::take(&mut self.yank.yanks) {
            if let Err(e) = self.client.set_buffer(None, &text, false).await {
                self.say(format!("set-buffer: {e}"));
            }
        }
    }

    async fn handle_event(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.pacing.last_key = Some(Instant::now());
                self.pacing.key_frames = TYPING_FRAMES;
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
        let text = text.into();
        self.chrome.message_log.push(text.clone());
        if self.chrome.message_log.len() > MESSAGE_LOG_CAP {
            self.chrome.message_log.remove(0);
        }
        self.interaction.message = Some((text, Instant::now()));
        Flow::Continue
    }

    /// Shows a failed daemon request on the status row, as `<what>: <error>`.
    pub fn report<T, E: std::fmt::Display>(&mut self, what: &str, result: Result<T, E>) {
        if let Err(e) = result {
            self.say(format!("{what}: {e}"));
        }
    }

    /// The live message, if one was set within the last few seconds.
    pub fn live_message(&self) -> Option<&str> {
        self.interaction
            .message
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_millis(self.config.display_time_ms))
            .map(|(text, _)| text.as_str())
    }

    pub fn remember(&mut self, line: &str) {
        if self.interaction.history.last().map(String::as_str) != Some(line) {
            self.interaction.history.push(line.to_string());
            if self.interaction.history.len() > HISTORY_CAP {
                self.interaction.history.remove(0);
            }
        }
    }

    pub fn repeating(&self) -> bool {
        self.interaction
            .repeat_until
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn arm_repeat(&mut self) {
        self.interaction.repeat_until = Some(Instant::now() + REPEAT_WINDOW);
    }

    /// Enters prefix mode; the shortcut popup follows after the configured delay.
    pub fn enter_prefix(&mut self) {
        self.mode = Mode::Prefix;
        self.interaction.prefix_at = Some(Instant::now());
    }

    /// When the pending shortcut popup is due, while prefix mode waits for its next key.
    pub fn hud_deadline(&self) -> Option<Instant> {
        let delay = Duration::from_millis(self.config.which_key_delay_ms);
        match self.mode {
            Mode::Prefix => self.interaction.prefix_at.map(|at| at + delay),
            _ => None,
        }
    }

    /// Whether the shortcut popup shows: prefix mode has lasted at least the configured delay.
    pub fn hud_visible(&self) -> bool {
        self.hud_deadline().is_some_and(|due| Instant::now() >= due)
    }

    /// When the display-panes numbers go away on their own; `None` while they wait for a key.
    pub fn display_deadline(&self) -> Option<Instant> {
        match self.mode {
            Mode::Panes { until } => until,
            _ => None,
        }
    }

    /// Clears the display-panes numbers once their time has passed at `now`.
    pub fn expire_panes(&mut self, now: Instant) {
        if let Mode::Panes { until: Some(until) } = self.mode
            && now >= until
        {
            self.mode = Mode::Normal;
            self.layout.dirty = true;
        }
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Flow {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key).await,
            Mode::Prefix => self.prefix_key(key).await,
            Mode::Copy(cs) => self.copy_key(cs, key).await,
            Mode::Overlay(ov) => self.overlay_key(ov, key).await,
            Mode::Prompt(p) => self.prompt_key(p, key).await,
            Mode::Confirm(prompt, cmd) => self.confirm_key(prompt, cmd, key).await,
            Mode::Panes { .. } => self.panes_key(key).await,
            // The mode is already back to normal: any key ends clock mode and is not forwarded.
            Mode::Clock { .. } => Flow::Continue,
            Mode::Popup(popup) => self.popup_key(popup, key).await,
            Mode::Menu(menu) => self.menu_key(menu, key).await,
        }
    }
}

/// Creates a shell window when the session has no running pane.
async fn ensure_running_window(client: &mut Client) -> Result<(), String> {
    let has_running = client
        .list()
        .await
        .map(|nodes| nodes.iter().any(|n| n.is_running()))
        .unwrap_or(false);
    if has_running {
        return Ok(());
    }
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let spawn = SpawnOpts {
        command: String::new(),
        pty: true,
        cwd,
        ..SpawnOpts::default()
    };
    client
        .new_window(NewWindowOpts {
            name: None,
            spawn,
            focus: true,
        })
        .await
        .map(|_| ())
}

/// This TUI as its session's client: the name it reports, sized when it attaches.
fn terminal_info(session: &str, client_name: Option<String>, read_only: bool) -> ClientInfo {
    let now = crate::node::now_ms();
    ClientInfo {
        name: client_name.unwrap_or_else(crate::clients::terminal_name),
        session: session.to_string(),
        width: 0,
        height: 0,
        readonly: read_only,
        created_ms: now,
        activity_ms: now,
        parent_pid: crate::clients::parent_pid(),
    }
}

/// When to draw output that arrived at `now`: a frame already scheduled stands; otherwise at once,
/// unless the budget is spent: `window_start` is the oldest of the last `FRAMES_PER_BUDGET` output
/// frames when that many were drawn. No settle delay: events that arrived together are already one
/// batch, and timers are too coarse (whole milliseconds) to wait for less than that.
pub fn next_frame(
    window_start: Option<Instant>,
    now: Instant,
    pending: Option<Instant>,
) -> Instant {
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
pub fn latest_drag(
    first: Event,
    mut queued: impl FnMut() -> Option<Event>,
) -> (Event, Option<Event>) {
    let is_drag =
        |ev: &Event| matches!(ev, Event::Mouse(m) if matches!(m.kind, MouseEventKind::Drag(_)));
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

/// Removes the run and detach events from `batch`: the lines to run, and whether the client was
/// detached.
fn take_control_events(batch: &mut Vec<(u64, StreamEvent)>) -> (Vec<String>, bool) {
    let mut runs = Vec::new();
    let mut detached = false;
    batch.retain(|(_, event)| match event {
        StreamEvent::Detached => {
            detached = true;
            false
        }
        StreamEvent::Run { line } => {
            runs.push(line.clone());
            false
        }
        _ => true,
    });
    (runs, detached)
}

/// Whether to draw after a wake-up. Pane output waits for its frame unless that frame is already
/// due (drawn now rather than through a timer, which would add up to a millisecond); a key that
/// only went to a pane changes nothing until the pane answers; anything else shows at once.
pub fn needs_redraw(
    output: bool,
    forwarded: bool,
    frame_due: Option<Instant>,
    now: Instant,
) -> bool {
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
    /// A command that waited on the daemon finished.
    Job(Job),
    /// The command of the open popup produced output or ended.
    Popup(PopupEvent),
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
    let mut tui = Tui::attach(endpoint, session, start, opts).await?;
    while tui.step().await {}
    Ok(())
}

/// The attached TUI: the app, its terminal and the buffers the event loop draws with.
struct Tui {
    // Fields drop in declaration order: the output flushes before the terminal guard restores
    // the screen, so `guard` stays last.
    out: BufWriter<std::io::Stdout>,
    body: Vec<u8>,
    cursor: Vec<u8>,
    last_cursor: Vec<u8>,
    tick: tokio::time::Interval,
    keys: Input,
    stream: StreamRx,
    app: App,
    guard: TerminalGuard,
    mouse_captured: bool,
    redraw: bool,
}

impl Tui {
    /// Enters the terminal and attaches the app to the session.
    async fn attach(
        endpoint: &str,
        session: &str,
        start: StartView,
        opts: AppOptions,
    ) -> Result<Self, String> {
        let guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {e}"))?;
        let report = super::terminal::query_terminal(opts.config.style.dim_inactive);
        let (mut app, stream) = App::open(endpoint, session, start, opts).await?;
        app.canvas.set_lr_margins(report.margins);
        app.host.colors = super::render::HostColors::from_report(&report);
        app.host.original = app.host.colors;
        let mut tui = Self {
            out: BufWriter::with_capacity(64 * 1024, stdout()),
            body: Vec::with_capacity(64 * 1024),
            cursor: Vec::new(),
            last_cursor: Vec::new(),
            tick: tokio::time::interval(Duration::from_secs(1)),
            keys: Input {
                rx: terminal_input(),
                held: None,
            },
            stream,
            app,
            guard,
            mouse_captured: true,
            redraw: true,
        };
        tui.sync_outer();
        Ok(tui)
    }

    /// One pass of the event loop: draw if due, wait for a wake-up, handle it. Returns false
    /// once the loop should end.
    async fn step(&mut self) -> bool {
        if self.redraw && !self.draw().await {
            return false;
        }
        let wake = self.wait().await;
        let output = matches!(wake, Wake::Stream(_) | Wake::Popup(_));
        let ticked = matches!(wake, Wake::Tick);
        let flow = self.handle(wake).await;
        self.sync_outer();
        let suspended = self.suspend_if_requested();
        sync_mouse_capture(
            &mut self.out,
            self.app.config.mouse,
            &mut self.mouse_captured,
        );
        let forwarded = std::mem::take(&mut self.app.pacing.forwarded);
        self.redraw = needs_redraw(output, forwarded, self.app.pacing.frame_due, Instant::now())
            || suspended
            || self.app.outer.bell_pending;
        if ticked
            && self.app.live_message().is_none()
            && self.app.pacing.frame_due.is_none()
            && !statusline::status_due(
                self.app.config.status_interval,
                self.app.pacing.drawn_at,
                Instant::now(),
            )
        {
            self.redraw = false;
        }
        flow != Flow::Quit
    }

    /// Draws the next frame, sending only what changed. Returns false when nothing is left to show.
    async fn draw(&mut self) -> bool {
        if !self.app.prepare_frame().await && !self.app.switch_after_destroy().await {
            return false;
        }
        self.body.clear();
        self.cursor.clear();
        let _ = frame::draw_body(&mut self.app, &mut self.body);
        let _ = frame::place_cursor(&self.app, &mut self.cursor);
        // A frame with no changes that leaves the cursor where it is would only be bytes for
        // the terminal to parse.
        if !self.body.is_empty() || self.cursor != self.last_cursor {
            let _ = frame::send(&mut self.out, &self.body, &self.cursor);
            std::mem::swap(&mut self.cursor, &mut self.last_cursor);
        }
        self.write_outer();
        self.app.drew(Instant::now());
        true
    }

    /// Waits for input, stream output, the tick or a delayed element coming due.
    async fn wait(&mut self) -> Wake {
        let popup = self.app.hud_deadline().filter(|_| !self.app.hud_visible());
        let due = earliest([
            popup,
            self.app.pacing.frame_due,
            self.app.display_deadline(),
            self.app.sync_deadline(),
        ]);
        tokio::select! {
            Some(event) = super::popup::next_event(&mut self.app.mode) => Wake::Popup(event),
            Some(event) = self.keys.recv() => Wake::Input(event),
            item = self.stream.recv() => Wake::Stream(item),
            _ = self.tick.tick() => Wake::Tick,
            _ = sleep_until_opt(due.map(tokio::time::Instant::from_std)) => Wake::Due,
            Some(job) = self.app.job_rx.recv() => Wake::Job(job),
        }
    }

    async fn handle(&mut self, wake: Wake) -> Flow {
        match wake {
            Wake::Input(event) => self.app.on_event(event).await,
            Wake::Stream(item) => {
                let flow = self.app.on_stream(item, &mut self.stream).await;
                flush_passthroughs(&mut self.out, &mut self.app.passthroughs);
                flow
            }
            Wake::Popup(event) => {
                self.app.on_popup(event);
                Flow::Continue
            }
            Wake::Tick => self.app.on_tick().await,
            Wake::Job(job) => self.app.finish_job(job).await,
            Wake::Due => {
                self.app.expire_panes(Instant::now());
                Flow::Continue
            }
        }
    }

    /// Gives the terminal to the shell when `suspend-client` asked for it; returns whether it did.
    fn suspend_if_requested(&mut self) -> bool {
        let suspended = std::mem::take(&mut self.app.chrome.suspend_requested);
        if suspended {
            let _ = self.out.flush();
            if let Err(e) = self.guard.suspend() {
                self.app.say(format!("suspend-client: {e}"));
            }
            self.app.canvas.invalidate();
            self.app.layout.dirty = true;
            self.last_cursor.clear();
            self.mouse_captured = true;
        }
        suspended
    }

    /// Brings the outer terminal's focus reporting and keyboard protocol in line with the options.
    fn sync_outer(&mut self) {
        let keys = self.app.config.terminal.extended_keys != ExtendedKeys::Off;
        let _ = self
            .guard
            .sync(&mut self.out, self.app.config.terminal.focus_events, keys);
        let _ = self.out.flush();
    }

    /// Sends the outer terminal the title and bell that the frame just drew calls for.
    fn write_outer(&mut self) {
        if let Some(title) = self.app.outer_title() {
            let _ = self.guard.set_title(&mut self.out, &title);
        }
        if self.app.take_bell() {
            let _ = self.out.write_all(b"\x07");
        }
        let _ = self.out.flush();
    }
}

/// Turns the terminal's mouse reporting on or off to match `mouse`; `captured` is what it is set to.
pub fn sync_mouse_capture<W: Write>(out: &mut W, want: bool, captured: &mut bool) {
    if want == *captured {
        return;
    }
    if want {
        let _ = crossterm::queue!(out, crossterm::event::EnableMouseCapture);
    } else {
        let _ = crossterm::queue!(out, crossterm::event::DisableMouseCapture);
    }
    let _ = out.flush();
    *captured = want;
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
