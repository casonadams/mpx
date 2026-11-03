use crate::keys::TermModes;
use crate::layout::{Direction, Divider, Rect, WindowTab};
use crate::vt::capture::CaptureSpec;
use serde::{Deserialize, Serialize};

/// Stable numeric pane identifier, rendered as `%N` (tmux-compatible).
pub type PaneId = u32;

/// A paste buffer: tmux's stack of copied text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Buffer {
    pub name: String,
    pub data: String,
}

/// One variable of the session environment: `Some(value)` is set, `None` marks a variable that is
/// kept out of new panes (`set-environment -r`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub name: String,
    pub value: Option<String>,
}

/// How `set-environment` changes one variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvChange {
    Set(String),
    /// Drops the variable from the session environment (`set-environment -u`).
    Unset,
    /// Keeps the variable out of new panes, even one mpx inherited (`set-environment -r`).
    Remove,
}

/// Options of `paste-buffer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasteBufferOpts {
    /// The buffer to paste; the newest one when `None`.
    pub name: Option<String>,
    /// The pane to paste into.
    pub pane: String,
    /// Deletes the buffer after pasting (`-d`).
    #[serde(default)]
    pub delete: bool,
    /// Brackets the paste even when the program did not ask for bracketed paste (`-p`).
    #[serde(default)]
    pub bracketed: bool,
    /// Line feeds are pasted as they are, not as carriage returns (`-r`).
    #[serde(default)]
    pub raw: bool,
    /// Replaces each line feed with this text (`-s`).
    #[serde(default)]
    pub separator: Option<String>,
}

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
    /// Pid of the PTY foreground process group leader (absent from daemons that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_pid: Option<u32>,
    /// Device name of the pane's terminal without `/dev/`, e.g. `ttys004`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tty: Option<String>,
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
    /// Column and row, zero-based, of a terminal pane's screen cursor (absent from daemons that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<(u16, u16)>,
    /// Scrollback lines a terminal pane's captures can reach (tmux `history_size`; absent from daemons that predate it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_size: Option<usize>,
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
            foreground_pid: None,
            tty: None,
            started_ms: 0,
            last_output_ms: 0,
            window: None,
            modes: None,
            cursor: None,
            history_size: None,
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
    MainVerticalMirrored,
    MainHorizontal,
    MainHorizontalMirrored,
    Tiled,
}

impl LayoutPreset {
    pub const ALL: [LayoutPreset; 7] = [
        LayoutPreset::EvenHorizontal,
        LayoutPreset::EvenVertical,
        LayoutPreset::MainVertical,
        LayoutPreset::MainVerticalMirrored,
        LayoutPreset::MainHorizontal,
        LayoutPreset::MainHorizontalMirrored,
        LayoutPreset::Tiled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LayoutPreset::EvenHorizontal => "even-horizontal",
            LayoutPreset::EvenVertical => "even-vertical",
            LayoutPreset::MainVertical => "main-vertical",
            LayoutPreset::MainVerticalMirrored => "main-vertical-mirrored",
            LayoutPreset::MainHorizontal => "main-horizontal",
            LayoutPreset::MainHorizontalMirrored => "main-horizontal-mirrored",
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
pub const PROTOCOL: u32 = 11;

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
    /// Node (pane process) requests.
    Node(NodeRequest),
    /// Layout of panes and windows, focus and marks.
    Pane(PaneRequest),
    Window(WindowRequest),
    Buffer(BufferRequest),
    Session(SessionRequest),
}

/// Requests about one node's process and its input and output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum NodeRequest {
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
    Screen {
        name: String,
    },
    Status {
        name: String,
    },
    Cwd {
        name: String,
    },
    /// Captures a pane's screen and history as text (`capture-pane`).
    Capture {
        name: String,
        spec: CaptureSpec,
    },
    /// Resize the PTY of a pane (client viewport change).
    Resize {
        name: String,
        cols: u16,
        rows: u16,
    },
    /// Restarts a pane's process in place (tmux `respawn-pane`): the pane keeps its name and
    /// layout slot. `kill` stops a running process first. `cwd` and `command` (program, then its
    /// arguments) replace the pane's own for this and later respawns.
    Respawn {
        name: String,
        #[serde(default)]
        kill: bool,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        command: Option<Vec<String>>,
    },
}

/// Requests that arrange panes within and across windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PaneRequest {
    Split(SplitOpts),
    FocusNext,
    /// `keep_zoom` (tmux `-Z`) leaves a zoomed window zoomed; otherwise focusing another pane
    /// unzooms it.
    FocusLast {
        #[serde(default)]
        keep_zoom: bool,
    },
    Focus {
        name: String,
        #[serde(default)]
        keep_zoom: bool,
    },
    /// Zooms or unzooms the window of pane `pane` (the active pane when `None`), focusing that
    /// pane first (tmux `resize-pane -Z`). `on` sets the state; `None` toggles it.
    Zoom {
        #[serde(default)]
        pane: Option<String>,
        #[serde(default)]
        on: Option<bool>,
    },
    /// Move the split border of a pane by `cells` towards `side` (tmux resize-pane -L/-R/-U/-D).
    ResizeSplit {
        name: String,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    },
    /// Exchanges two panes. `keep_zoom` (tmux `-Z`) leaves a zoomed window zoomed.
    SwapPane {
        #[serde(default)]
        a: Option<String>,
        #[serde(default)]
        b: Option<String>,
        #[serde(default)]
        keep_zoom: bool,
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
    /// Moves pane `src` next to `dst`, splitting `dst` side by side or stacked (tmux `join-pane`).
    Join {
        src: String,
        dst: String,
        side_by_side: bool,
    },
    SelectLayout {
        /// `None` cycles to the next preset.
        preset: Option<LayoutPreset>,
    },
    /// Spreads the active window's panes evenly, keeping their arrangement (`select-layout -E`).
    SpreadLayout,
    /// Turns `synchronize-panes` on or off for window `window` (the active window when `None`);
    /// `on` None toggles it.
    Synchronize {
        #[serde(default)]
        window: Option<usize>,
        #[serde(default)]
        on: Option<bool>,
    },
}

/// Requests about the session's windows: creating, selecting, renaming, reordering and killing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum WindowRequest {
    New(NewWindowOpts),
    Next,
    Prev,
    Last,
    Select {
        index: usize,
    },
    /// `index: None` targets the active window.
    Rename {
        index: Option<usize>,
        name: String,
    },
    Kill {
        index: Option<usize>,
    },
    Move {
        from: usize,
        to: usize,
    },
    /// Renumbers the windows 0, 1, ... in order whatever `renumber-windows` says (tmux `move-window -r`).
    Renumber,
    /// Rotates the panes of the active window through its layout slots (tmux `rotate-window`).
    /// The default moves every pane to the slot before it (the first pane goes last); `down`
    /// moves every pane to the slot after it. Pane names move, slots and focus stay.
    Rotate {
        down: bool,
    },
    /// Exchanges the numbers of two windows (tmux `swap-window`).
    Swap {
        a: usize,
        b: usize,
    },
}

/// Requests about paste buffers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum BufferRequest {
    /// Adds a paste buffer: `name` replaces the buffer of that name, else a new `bufferNNNN` one.
    /// With `append` (`set-buffer -a`), `data` is added to the end of the named buffer (the newest
    /// one when unnamed), or starts a buffer when there is none.
    Set {
        name: Option<String>,
        data: String,
        #[serde(default)]
        append: bool,
    },
    /// The named paste buffer, or the most recent one.
    Get {
        name: Option<String>,
    },
    List,
    Delete {
        name: Option<String>,
    },
    /// Renames a buffer (`set-buffer -n`); `name` None is the newest buffer.
    Rename {
        name: Option<String>,
        to: String,
    },
    /// Pastes a buffer into a pane (`paste-buffer`).
    Paste(PasteBufferOpts),
}

/// Requests about the session itself: its name, config, key bindings, environment and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SessionRequest {
    Rename {
        name: String,
    },
    /// Reload the daemon's config from `path`, or the defaults when there is none.
    ReloadConfig {
        path: Option<String>,
    },
    /// Set a key binding made while running (`bind-key`): `name` is `prefix:KEY` or `root:KEY`
    /// (or a copy-mode table), `line` the command line, empty for a removed binding; `None` forgets
    /// it. Terminals are told to refresh with `StreamEvent::Bindings`.
    SetBinding {
        name: String,
        line: Option<String>,
    },
    /// All runtime key bindings, sorted by name.
    GetBindings,
    /// Sets, removes or unsets one variable of the session environment (`set-environment`).
    SetEnvironment {
        name: String,
        change: EnvChange,
    },
    /// The session environment, or one variable of it (`show-environment`).
    ShowEnvironment {
        name: Option<String>,
    },
    /// Resizes the session's viewport to `width` by `height` cells.
    Layout {
        width: u16,
        height: u16,
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
    Bindings(Vec<(String, String)>),
    Buffer(Buffer),
    Buffers(Vec<Buffer>),
    /// Text captured by `capture-pane`.
    Capture(String),
    /// Session environment entries, sorted by name.
    Environment(Vec<EnvEntry>),
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
    /// The session's runtime key bindings changed: terminals re-read them.
    Bindings,
    /// A pane's process was replaced under the same name (`respawn-pane`): its output stream
    /// starts over, so terminals drop the screen they hold. Sent only to subscribers that asked
    /// for layout events.
    Respawned {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonMessage {
    Response(Response),
    Event(StreamEvent),
}
