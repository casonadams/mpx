use serde::{Deserialize, Serialize};
use crate::layout::{Direction, Divider, Rect, WindowTab};

/// Stable numeric pane identifier, rendered as `%N` (tmux-compatible).
pub type PaneId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Running,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    NeedsInput,
    Working,
    Idle,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub harness: String,
    pub state: AgentState,
    pub session_id: Option<String>,
    pub detail: Option<String>,
    /// Unix epoch milliseconds of the last state transition.
    pub since_ms: u64,
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
    pub agent: Option<AgentInfo>,
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
            agent: None,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Agent reached the given state.
    State { state: AgentState },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitOutcome {
    /// False when the timeout elapsed first.
    pub satisfied: bool,
    pub exit_code: Option<i32>,
    pub state: Option<AgentState>,
    pub matched: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeMessage {
    pub seq: u64,
    pub ts_ms: u64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeInfo {
    pub name: String,
    pub messages: usize,
    pub next_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub endpoint: String,
    pub pid: u32,
    pub created_ms: u64,
    pub windows: usize,
    pub panes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    Ping,
    Info,
    Shutdown,
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
    Subscribe {
        name: Option<String>,
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
        a: String,
        b: String,
    },
    BreakPane {
        name: String,
    },
    SelectLayout {
        /// `None` cycles to the next preset.
        preset: Option<LayoutPreset>,
    },
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
    CreatePipe {
        name: String,
        capacity: Option<usize>,
    },
    /// Appends one message; creates the pipe if missing.
    WritePipe {
        name: String,
        data: String,
    },
    /// `consumer` keeps a daemon-side cursor; otherwise `since` (exclusive seq) or from the start.
    /// `wait_ms` blocks until at least one message is available or the timeout elapses.
    ReadPipe {
        name: String,
        #[serde(default)]
        since: Option<u64>,
        #[serde(default)]
        consumer: Option<String>,
        #[serde(default)]
        wait_ms: Option<u64>,
        #[serde(default)]
        limit: Option<usize>,
    },
    ListPipes,
    SubscribePipe {
        name: String,
    },
    Status {
        name: String,
    },
    /// Reported by harness hooks (`mpx hook`). `clear` drops agent tracking for the pane.
    AgentReport {
        name: String,
        #[serde(default)]
        harness: Option<String>,
        #[serde(default)]
        state: Option<AgentState>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        detail: Option<String>,
        #[serde(default)]
        clear: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    Ok,
    Pong,
    Session(SessionInfo),
    Node(NodeInfo),
    NodeList(Vec<NodeInfo>),
    LogChunk { data: String, next: u64 },
    WaitResult(WaitOutcome),
    Error(String),
    Screen(String),
    Layout(Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>),
    PipeSeq(u64),
    PipeMessages { messages: Vec<PipeMessage>, next: u64 },
    PipeList(Vec<PipeInfo>),
    Cwd(Option<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum StreamEvent {
    Output { name: String, data: String },
    Exit { name: String, code: Option<i32> },
    Spawned { name: String, pid: Option<u32> },
    Pipe { name: String, message: PipeMessage },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonMessage {
    Response(Response),
    Event(StreamEvent),
}
