use crate::keys::TermModes;
use crate::layout::{Direction, Divider, Rect, WindowTab};
use serde::{Deserialize, Serialize};

/// Stable numeric pane identifier, rendered as `%N` (tmux-compatible).
pub type PaneId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub id: PaneId,
    pub pid: Option<u32>,
    pub pty: bool,
    pub command: String,
    pub state: NodeState,
    pub exit_code: Option<i32>,
    pub cwd: Option<String>,
    /// Title set by the program via OSC 0/2.
    pub title: Option<String>,
    /// Name of the PTY foreground process (e.g. `claude` running inside `zsh`).
    pub foreground: Option<String>,
    pub started_ms: u64,
    pub last_output_ms: u64,
    /// Index of the window containing this pane, if it is laid out.
    pub window: Option<usize>,
    /// Input modes the program in a terminal pane has set (absent from daemons that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<TermModes>,
    /// Size (cols, rows) of a terminal pane's PTY (absent from daemons that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(u16, u16)>,
    /// Whether this pane is the marked pane in the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marked: Option<bool>,
}

impl NodeInfo {
    /// Running node with every optional field empty.
    pub fn new(name: String, pty: bool, command: String) -> Self {
        Self {
            name,
            id: 0,
            pid: None,
            pty,
            command,
            state: NodeState::Running,
            exit_code: None,
            cwd: None,
            title: None,
            foreground: None,
            started_ms: 0,
            last_output_ms: 0,
            window: None,
            modes: None,
            size: None,
            marked: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.state == NodeState::Running
    }

    pub fn pane_ref(&self) -> String {
        format!("%{}", self.id)
    }

    pub fn status_label(&self) -> String {
        match (self.state, self.exit_code) {
            (NodeState::Running, _) => "running".to_string(),
            (NodeState::Exited, Some(c)) => format!("exited ({})", c),
            (NodeState::Exited, None) => "exited".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpawnOpts {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub pty: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Keep the pane on screen after exit; `None` uses the daemon config default.
    #[serde(default)]
    pub remain_on_exit: Option<bool>,
    /// Export TMUX/TMUX_PANE and put the mpx `tmux` shim first on PATH.
    #[serde(default)]
    pub tmux_compat: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitOpts {
    pub target: Option<String>,
    pub spawn: SpawnOpts,
    pub direction: Direction,
    /// Size of the new pane as a percentage of the split target (tmux `-l N%`).
    #[serde(default)]
    pub percent: Option<u8>,
    #[serde(default = "default_true")]
    pub focus: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWindowOpts {
    /// Window label; also used to derive the node name when `spawn.name` is empty.
    pub name: Option<String>,
    pub spawn: SpawnOpts,
    #[serde(default = "default_true")]
    pub focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutPreset {
    EvenHorizontal,
    EvenVertical,
    MainVertical,
    MainHorizontal,
    Tiled,
}

impl LayoutPreset {
    pub const ALL: [LayoutPreset; 5] = [
        LayoutPreset::EvenHorizontal,
        LayoutPreset::EvenVertical,
        LayoutPreset::MainVertical,
        LayoutPreset::MainHorizontal,
        LayoutPreset::Tiled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LayoutPreset::EvenHorizontal => "even-horizontal",
            LayoutPreset::EvenVertical => "even-vertical",
            LayoutPreset::MainVertical => "main-vertical",
            LayoutPreset::MainHorizontal => "main-horizontal",
            LayoutPreset::Tiled => "tiled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitCondition {
    /// Process exit.
    Exit,
    /// No output for `quiet_ms` milliseconds.
    Idle { quiet_ms: u64 },
    /// Regex matched against plain text (current screen plus output produced after the request).
    Match { pattern: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitOutcome {
    /// False when the timeout elapsed first.
    pub satisfied: bool,
    pub exit_code: Option<i32>,
    pub matched: Option<String>,
}


/// Version of the client-daemon protocol. Bump it on any change an older peer cannot parse, so
/// clients refuse an incompatible daemon instead of misreading it.
pub const PROTOCOL: u32 = 2;

fn first_protocol() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub endpoint: String,
    pub pid: u32,
    pub created_ms: u64,
    pub windows: usize,
    pub panes: usize,
    /// Unix ms of the snapshot this session was restored from, if any.
    #[serde(default)]
    pub restored_ms: Option<u64>,
    /// Protocol the daemon speaks; daemons that predate the field speak version 1.
    #[serde(default = "first_protocol")]
    pub protocol: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    Ping,
    Info,
    Shutdown,
    /// Recreate windows and panes from the session's last snapshot. Errors when the
    /// session already has panes or no snapshot exists.
    Restore,
    /// Run `request` at most once per `key`: a repeated key returns the first response (also while
    /// the first is still running).
    Idempotent {
        key: String,
        request: Box<Request>,
    },
    Spawn(SpawnOpts),
    List,
    /// Read output. `since` is an absolute byte offset returned as `next` by a previous call.
    /// `plain` renders PTY scrollback as text (or strips escapes from a `since` delta).
    Logs {
        name: String,
        tail: Option<usize>,
        #[serde(default)]
        plain: bool,
        #[serde(default)]
        since: Option<u64>,
    },
    /// Raw bytes to stdin.
    Send {
        name: String,
        data: String,
    },
    /// tmux key names (`Enter`, `C-c`, `S-Tab`, `M-x`, `F5`); `literal` sends them as text.
    SendKeys {
        name: String,
        keys: Vec<String>,
        #[serde(default)]
        literal: bool,
    },
    /// Paste text (bracketed when the program enabled ?2004); `submit` presses Enter afterwards.
    Paste {
        name: String,
        text: String,
        #[serde(default)]
        submit: bool,
    },
    Kill {
        name: String,
    },
    Wait {
        name: String,
        condition: WaitCondition,
        timeout_ms: Option<u64>,
    },
    /// Streams events. With `name` and `since`, output from byte offset `since` is replayed first,
    /// with no gap before live output.
    Subscribe {
        name: Option<String>,
        #[serde(default)]
        since: Option<u64>,
        /// Ask for events as binary frames (`wire`); a daemon that agrees answers `BinaryStream`.
        #[serde(default)]
        binary: bool,
        /// Of pane output, send only the active window's: what a TUI shows. Other events still
        /// come for every pane.
        #[serde(default)]
        active_window: bool,
        /// With `active_window`: also send `Layout` whenever windows or panes are rearranged.
        #[serde(default)]
        layout: bool,
    },
    Screen {
        name: String,
    },
    Split(SplitOpts),
    Layout {
        width: u16,
        height: u16,
    },
    FocusNext,
    FocusLast,
    Focus {
        name: String,
    },
    /// Resize the PTY of a pane (client viewport change).
    ResizePane {
        name: String,
        cols: u16,
        rows: u16,
    },
    /// Move the split border of a pane by `cells` towards `side` (tmux resize-pane -L/-R/-U/-D).
    ResizeSplit {
        name: String,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    },
    SwapPane {
        #[serde(default)]
        a: Option<String>,
        #[serde(default)]
        b: Option<String>,
    },
    /// Mark or unmark a pane. If `name` is None, targets the currently active pane.
    /// If `clear` is true (-M), clears the marked pane unconditionally.
    MarkPane {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        clear: bool,
    },
    BreakPane {
        name: String,
    },
    SelectLayout {
        /// `None` cycles to the next preset.
        preset: Option<LayoutPreset>,
    },
    /// Spreads the active window's panes evenly, keeping their arrangement (`select-layout -E`).
    SpreadLayout,
    NewWindow(NewWindowOpts),
    NextWindow,
    PrevWindow,
    LastWindow,
    SelectWindow {
        index: usize,
    },
    /// `index: None` targets the active window.
    RenameWindow {
        index: Option<usize>,
        name: String,
    },
    KillWindow {
        index: Option<usize>,
    },
    GetCwd {
        name: String,
    },
    Status {
        name: String,
    },
    RenameSession {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    Ok,
    /// Subscribed; events follow as binary frames (`wire`) instead of JSON lines.
    BinaryStream,
    Pong,
    Session(SessionInfo),
    Node(NodeInfo),
    NodeList(Vec<NodeInfo>),
    LogChunk {
        data: String,
        next: u64,
    },
    WaitResult(WaitOutcome),
    Error(String),
    Screen(String),
    Layout(Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>),
    Cwd(Option<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum StreamEvent {
    /// `offset`/`next`: absolute byte offsets of this chunk in the pane's output stream. A chunk whose
    /// `offset` differs from the previous chunk's `next` means bytes were missed (fetch them with Logs `since`).
    Output {
        name: String,
        data: String,
        offset: u64,
        next: u64,
    },
    Exit {
        name: String,
        code: Option<i32>,
    },
    Spawned {
        name: String,
        pid: Option<u32>,
    },
    /// This subscriber fell behind and events were dropped; re-read state before trusting the stream.
    Lagged,
    /// Windows or panes were rearranged (focus, a window switch, a split, a swap...): refetch the
    /// layout. Sent only to subscribers that asked for it, so older clients never see it.
    Layout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonMessage {
    Response(Response),
    Event(StreamEvent),
}
