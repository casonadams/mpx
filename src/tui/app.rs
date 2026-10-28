use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::{BufWriter, stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::agent::AgentWatch;
use super::canvas::Canvas;
use super::command::Command;
use super::copy::CopyState;
use super::exec::default_shell;
use super::frame;
use super::keymap::{self, Keymap};
use super::overlay::Overlay;
use super::panes::{LayoutCache, PaneRect, sync_dims};
use super::prompt::PromptState;
use super::render::copy_to_clipboard;
use super::sidebar::{self, Sidebar};
use super::status::{local_utc_offset, restored_notice};
use super::terminal::TerminalGuard;
use crate::agents::{self, Harness};
use crate::client::Client;
use crate::config::Config;
use crate::keys::TermModes;
use crate::layout::geometry::Rect;
use crate::protocol::{NewWindowOpts, NodeInfo, SpawnOpts, StreamEvent};
use crate::vt::VtScreen;

/// Initial screen when attaching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartView {
    Panes,
    Agents,
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
    /// Keys go to the agents sidebar.
    Sidebar,
}

/// Event-stream subscription for the attached session. Events tagged with an older
/// generation come from a previous session and are dropped.
pub struct Subscription {
    pub generation: u64,
    pub tx: UnboundedSender<(u64, StreamEvent)>,
    pub task: Option<JoinHandle<()>>,
}

pub type StreamRx = UnboundedReceiver<(u64, StreamEvent)>;

const FRAME_BUDGET: Duration = Duration::from_millis(16);
const REPEAT_WINDOW: Duration = Duration::from_millis(500);
const MESSAGE_TTL: Duration = Duration::from_secs(3);
const HISTORY_CAP: usize = 100;

pub struct App {
    pub endpoint: String,
    pub session: String,
    pub client: Client,
    pub config: Config,
    pub keymap: Keymap,
    pub registry: Vec<Harness>,
    pub mode: Mode,
    pub history: Vec<String>,
    pub message: Option<(String, Instant)>,
    pub repeat_until: Option<Instant>,
    /// When the prefix key was pressed; the shortcut popup waits `which_key_delay_ms` after it.
    pub prefix_at: Option<Instant>,
    /// Pane and cell of a left-button press; a drag from it starts a copy-mode selection.
    pub press: Option<(String, usize, usize)>,
    /// Copy-mode state of panes that are not in the foreground: a pane keeps its scroll position,
    /// cursor and selection while the prefix is pending or focus moves elsewhere.
    pub parked_copy: HashMap<String, CopyState>,
    pub vt: HashMap<String, VtScreen>,
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
    pub focus_pane: Option<String>,
    pub zoomed: Option<String>,
    pub split_counter: usize,
    /// Nodes of the attached session, refreshed every tick.
    pub nodes: Vec<NodeInfo>,
    pub watch: AgentWatch,
    pub sub: Subscription,
    pub last_render: Option<Instant>,
    /// Directory holding the session sockets, used to discover and switch sessions.
    pub socket_dir: PathBuf,
    /// Fixed screen size (columns, rows) instead of the real terminal's.
    pub fixed_size: Option<(u16, u16)>,
    /// Where yanks go; `Captured` keeps them in memory instead of the system clipboard.
    pub clipboard: Clipboard,
    /// What the terminal currently shows, so frames send only changed cells.
    pub canvas: Canvas,
    pub sidebar: Sidebar,
}

/// Everything the app reads from its surroundings, so it can run against explicit inputs.
#[derive(Debug, Clone)]
pub struct AppOptions {
    pub config: Config,
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
        start: StartView,
        opts: AppOptions,
    ) -> Result<(Self, StreamRx), String> {
        let AppOptions {
            config,
            socket_dir,
            size,
        } = opts;
        let keymap = keymap::resolve(&config)?;
        let mut client = Client::connect(endpoint).await?;
        let sub_client = Client::connect(endpoint).await?;
        ensure_running_window(&mut client).await;
        let (tx, rx) = unbounded_channel();
        let registry = agents::registry(&config);
        let mut app = App {
            endpoint: endpoint.to_string(),
            session: session.to_string(),
            client,
            config,
            keymap,
            registry,
            mode: Mode::Normal,
            history: Vec::new(),
            message: None,
            repeat_until: None,
            prefix_at: None,
            press: None,
            parked_copy: HashMap::new(),
            vt: HashMap::new(),
            pane_dims: HashMap::new(),
            expected: HashMap::new(),
            layout: LayoutCache::new(),
            viewport: (80, 23),
            panes: Vec::new(),
            active_pane: String::new(),
            active_rect: None,
            focus_pane: None,
            zoomed: None,
            split_counter: 1,
            nodes: Vec::new(),
            watch: AgentWatch::default(),
            sub: Subscription {
                generation: 0,
                tx,
                task: None,
            },
            last_render: None,
            socket_dir,
            fixed_size: size,
            clipboard: Clipboard::System,
            canvas: Canvas::default(),
            sidebar: Sidebar::default(),
        };
        app.subscribe(sub_client);
        app.refresh_nodes().await;
        app.refresh_agents().await;
        if let Ok(info) = app.client.info().await
            && let Some(restored_ms) = info.restored_ms
        {
            app.say(restored_notice(restored_ms, local_utc_offset(restored_ms)));
        }
        if start == StartView::Agents {
            app.open_overlay(Overlay::agents()).await;
        }
        Ok((app, rx))
    }

    /// Replaces the event subscription with one bound to `client`'s session.
    pub fn subscribe(&mut self, mut client: Client) {
        if let Some(task) = self.sub.task.take() {
            task.abort();
        }
        self.sub.generation += 1;
        let generation = self.sub.generation;
        let tx = self.sub.tx.clone();
        self.sub.task = Some(tokio::spawn(async move {
            let _ = client
                .stream_logs(None, None, |event| tx.send((generation, event)).is_ok())
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
        let side = sidebar::width(self.sidebar.visible, w);
        if side == 0 && matches!(self.mode, Mode::Sidebar) {
            self.mode = Mode::Normal;
        }
        if !self.layout.sync(&mut self.client, side, w - side, layout_h).await {
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
        self.panes = match &self.zoomed {
            Some(name) => vec![(name.clone(), Rect::new(side, 0, w - side, layout_h), true)],
            None => self.layout.raw.clone(),
        };
        self.active_rect = self.panes.iter().find(|(.., f)| *f).map(|(_, r, _)| *r);
        sync_dims(
            &mut self.client,
            &self.panes,
            &mut self.pane_dims,
            &mut self.vt,
            &mut self.expected,
        )
        .await;
        self.route_focus().await;
        true
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
        let (w, h) = self.viewport;
        (w - self.sidebar_width(), h)
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

    /// Applies stream events from the current subscription. Returns Quit when no pane is left.
    pub async fn on_stream(
        &mut self,
        first: Option<(u64, StreamEvent)>,
        rx: &mut StreamRx,
    ) -> Flow {
        let Some(first) = first else {
            return Flow::Continue;
        };
        let mut batch = vec![first];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
        }
        for (generation, event) in batch {
            if generation == self.sub.generation && self.apply_event(event).await {
                self.layout.dirty = true;
            }
        }
        if self.vt.is_empty() {
            return Flow::Quit;
        }
        wait_for_frame(self.last_render, FRAME_BUDGET).await;
        self.last_render = Some(Instant::now());
        Flow::Continue
    }

    pub async fn on_tick(&mut self) -> Flow {
        self.layout.dirty = true;
        self.refresh_nodes().await;
        self.refresh_agents().await;
        if matches!(self.mode, Mode::Overlay(_)) {
            self.refresh_overlay().await;
        }
        Flow::Continue
    }

    pub async fn on_event(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key).await,
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
            Mode::Sidebar => self.sidebar_key(key).await,
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

async fn wait_for_frame(last_render: Option<Instant>, budget: Duration) {
    if let Some(last) = last_render
        && last.elapsed() < budget
    {
        tokio::time::sleep(budget.saturating_sub(last.elapsed())).await;
    }
}

/// One wake-up of the event loop; handlers run after the select has released its borrows.
enum Wake {
    Input(Event),
    Stream(Option<(u64, StreamEvent)>),
    Tick,
    /// A delayed element is due: the shortcut popup, or a sidebar switch to another session.
    Due,
}

/// Resolves at `deadline`, or never without one.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

/// Runs the attached TUI until detach, session end or no panes left.
pub async fn run_tui(
    endpoint: &str,
    session: &str,
    start: StartView,
    opts: AppOptions,
) -> Result<(), String> {
    let _guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {e}"))?;
    let (mut app, mut stream) = App::open(endpoint, session, start, opts).await?;
    let mut keys = EventStream::new();
    let mut out = BufWriter::with_capacity(64 * 1024, stdout());
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        if !app.prepare_frame().await {
            return Ok(());
        }
        let _ = frame::draw(&mut app, &mut out);
        let popup = app.hud_deadline().filter(|_| !app.hud_visible());
        let due = match (popup, app.sidebar.due()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let wake = tokio::select! {
            Some(Ok(event)) = keys.next() => Wake::Input(event),
            item = stream.recv() => Wake::Stream(item),
            _ = tick.tick() => Wake::Tick,
            _ = until(due) => Wake::Due,
        };
        let flow = match wake {
            Wake::Input(event) => app.on_event(event).await,
            Wake::Stream(item) => app.on_stream(item, &mut stream).await,
            Wake::Tick => app.on_tick().await,
            Wake::Due => app.on_due().await,
        };
        if flow == Flow::Quit {
            return Ok(());
        }
    }
}
