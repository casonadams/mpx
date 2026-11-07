use crate::keys::TermModes;
use crate::vt::capture::CaptureSpec;
use serde::{Deserialize, Serialize};

use super::PaneId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    #[default]
    Running,
    Exited,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground_pid: Option<u32>,
    /// Device name of the pane's terminal without `/dev/`, e.g. `ttys004`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tty: Option<String>,
    pub started_ms: u64,
    pub last_output_ms: u64,
    /// Index of the window containing this pane, if it is laid out.
    pub window: Option<usize>,
    /// Input modes the program in a terminal pane has set (absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<TermModes>,
    /// Size (cols, rows) of a terminal pane's PTY (absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<(u16, u16)>,
    /// Size (cols, rows) the pane's retained output was replayed from before its last resize, when that
    /// differs from `size`: a fresh screen replays from it and resizes to `size` (absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resized_from: Option<(u16, u16)>,
    /// Offset in the pane's output where its last resize happened: output before it was written at `resized_from`, output after it at `size` (absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resized_at: Option<u64>,
    /// Column and row, zero-based, of a terminal pane's screen cursor (absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<(u16, u16)>,
    /// Scrollback lines a terminal pane's captures can reach (tmux `history_size`; absent from daemons that predate it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_size: Option<usize>,
    /// Whether this pane is the marked pane in the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marked: Option<bool>,
    /// Whether input to this pane is off (tmux `select-pane -d`): typed and sent keys are dropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_off: Option<bool>,
}

impl NodeInfo {
    /// Running node with every optional field empty.
    pub fn new(name: String, pty: bool, command: String) -> Self {
        Self {
            name,
            pty,
            command,
            ..Self::default()
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
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Keep the pane on screen after exit; `None` uses the daemon config default.
    pub remain_on_exit: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum NodeRequest {
    Spawn(SpawnOpts),
    List,
    /// Read output. `since` is an absolute byte offset returned as `next` by a previous call; `until`
    /// instead reads the bytes before an offset (`next` is that offset).
    /// `plain` renders PTY scrollback as text (or strips escapes from a `since` delta).
    Logs {
        name: String,
        tail: Option<usize>,
        #[serde(default)]
        plain: bool,
        since: Option<u64>,
        until: Option<u64>,
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
    /// The pane's live screen as the escape sequences that rebuild it, with the stream offset it
    /// is current to.
    Keyframe {
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
        cwd: Option<String>,
        command: Option<Vec<String>>,
    },
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
