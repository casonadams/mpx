use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use tokio::sync::mpsc::{self, UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

use crate::client::Client;
use crate::config::Config;
use crate::layout::geometry::Rect;
use crate::protocol::{ClientInfo, NodeInfo, StreamEvent};
use crate::tui::canvas::Canvas;
use crate::tui::command::Command;
use crate::tui::copy::CopyState;
use crate::tui::job::Job;
use crate::tui::keymap::Keymap;
use crate::tui::menu::MenuState;
use crate::tui::outer::OuterTerminal;
use crate::tui::overlay::Overlay;
use crate::tui::panes::{BorderGrab, LayoutCache, PaneRect};
use crate::tui::popup::PopupState;
use crate::tui::prompt::PromptRun;
use crate::tui::render::{HostColors, copy_to_clipboard};
use crate::tui::status::{local_utc_offset, time_of_day};
use crate::tui::statusline::{self, Rows, Target};
use crate::tui::warm::Warm;
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

pub type StreamRx = mpsc::Receiver<(u64, StreamEvent)>;

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
    /// The focused pane's scrollback built ahead of copy mode (see `warm`).
    pub warm: Option<Warm>,
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

/// The host terminal's colors.
#[derive(Default)]
pub struct HostTerm {
    /// Default foreground and background of the host terminal, read at attach.
    pub colors: HostColors,
    /// What the terminal reported at attach, which a color reset goes back to.
    pub original: HostColors,
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
    /// The app before it is attached: its state from the given parts and options.
    pub(super) fn new(
        endpoint: &str,
        session: &str,
        client: Client,
        keymap: Keymap,
        tx: mpsc::Sender<(u64, StreamEvent)>,
        opts: AppOptions,
    ) -> Self {
        let (jobs, job_rx) = unbounded_channel();
        App {
            endpoint: endpoint.to_string(),
            session: session.to_string(),
            client,
            config: opts.config,
            keymap,
            mode: Mode::Normal,
            parked_copy: HashMap::new(),
            released: Vec::new(),
            vt: HashMap::new(),
            passthroughs: Vec::new(),
            pane_dims: HashMap::new(),
            expected: HashMap::new(),
            warm: None,
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
            sub: Subscription {
                generation: 0,
                tx,
                task: None,
            },
            canvas: Canvas::default(),
            jobs,
            job_rx,
            outer: OuterTerminal::default(),
            pacing: FramePacing::default(),
            interaction: Interaction::default(),
            launch: Launch {
                config_path: opts.config_path,
                socket_dir: opts.socket_dir,
                fixed_size: opts.size,
            },
            status_hits: StatusHits::default(),
            yank: YankState {
                clipboard: Clipboard::System,
                last_copy: None,
                yanks: Vec::new(),
            },
            host: HostTerm::default(),
            chrome: Chrome::default(),
            attachment: Attachment {
                terminal: terminal_info(session, opts.client_name, opts.attach.read_only),
                attached: 0,
                times: Default::default(),
                previous_session: None,
            },
        }
    }
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
