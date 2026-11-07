use serde::{Deserialize, Serialize};

use super::buffer::EnvChange;

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
    pub restored_ms: Option<u64>,
    /// Protocol the daemon speaks; daemons that predate the field speak version 1.
    #[serde(default = "first_protocol")]
    pub protocol: u32,
}

/// An attached terminal (tmux client), as the daemon of its session sees it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Terminal device path (`/dev/ttys004`), or a generated `client-PID-N` without a terminal.
    pub name: String,
    /// Session the client is attached to; the daemon fills it in.
    #[serde(default)]
    pub session: String,
    /// Size of the client's terminal in cells.
    pub width: u16,
    pub height: u16,
    /// Attached with `-r`: keys do not reach panes.
    #[serde(default)]
    pub readonly: bool,
    pub created_ms: u64,
    /// Unix ms of the client's latest input; the `latest` window-size policy follows it.
    #[serde(default)]
    pub activity_ms: u64,
    /// Process id of the client's parent (the shell that started it), which `attach -x` hangs up
    /// (tmux `attach -x`). Absent on Windows, where no such signal exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
}

/// Which attached clients of a session a request applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientTarget {
    /// The client with this name.
    Named(String),
    /// Every client except the one with this name.
    Others(String),
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ClientRequest {
    /// The session's attached clients.
    List,
    /// Detaches the clients `target` selects: each gets `StreamEvent::Detached` and its stream ends.
    Detach { target: ClientTarget },
    /// Redraws the client `name`; `size` (cols, rows) sets the size the window-size policy uses
    /// for it, until the terminal reports a new size.
    Refresh {
        name: String,
        size: Option<(u16, u16)>,
    },
    /// The client `name` received input: it becomes the most recently active client.
    Activity { name: String },
    /// The client `name`'s terminal is now `width` x `height` cells.
    Resized {
        name: String,
        width: u16,
        height: u16,
    },
    /// Sends the command line `line` to the client `name` to run as its own command (tmux
    /// `switch-client` and friends, run from outside the client).
    Command { name: String, line: String },
}

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

/// A `wait-for` operation on a named channel: `Wait` blocks until a `Signal`, `Lock` and `Unlock`
/// take and release its lock (tmux `wait-for`, `-S`, `-L` and `-U`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelOp {
    Wait,
    Signal,
    Lock,
    Unlock,
}

/// The events a hook can run on (`set-hook`), by their tmux names.
pub const HOOK_EVENTS: &[&str] = &[
    "after-new-window",
    "after-split-window",
    "after-kill-pane",
    "after-select-pane",
    "after-select-window",
    "pane-exited",
    "pane-died",
    "client-attached",
    "client-detached",
    "alert-bell",
    "alert-activity",
    "alert-silence",
    "after-new-session",
    "window-linked",
    "window-unlinked",
    "session-closed",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ScriptRequest {
    /// Runs `command` through the shell (tmux `run-shell`). `target` gives the working directory
    /// (the pane's; the daemon's without one). `delay_ms` waits first. A foreground run answers
    /// `Shell` with the exit status and output; `background` answers `Ok` at once.
    RunShell {
        command: String,
        target: Option<String>,
        #[serde(default)]
        background: bool,
        delay_ms: Option<u64>,
    },
    /// Runs one command line the way a hook runs its commands, with the session's aliases; the
    /// line's own `-t` targets, else the active pane. `if-shell` decides its branch and runs it.
    /// Answers `Ok` once the command has run, or `Error`.
    RunCommand { line: String },
    /// Expands the tmux formats in `fmt` for pane `target` (the active pane when `None`). With
    /// `hook`, the variables `hook` and `hook_pane` are set as well.
    Format {
        fmt: String,
        target: Option<String>,
        hook: Option<String>,
    },
    /// Signals, waits on or locks a named channel (tmux `wait-for`). `Wait` and `Lock` answer
    /// when the channel allows it.
    WaitFor { channel: String, op: ChannelOp },
    /// Sets the commands of hook `name` (`append` adds one after the existing commands); `None`
    /// removes the hook.
    SetHook {
        name: String,
        command: Option<String>,
        #[serde(default)]
        append: bool,
    },
    /// Runs the commands of hook `name` now, without an event (tmux `set-hook -R`).
    RunHook { name: String },
    /// Every hook with its command line, sorted by name then order.
    ShowHooks,
    /// Sets (or with `None` removes) a command alias: `alias` is the name, `command` its command
    /// line (tmux `set -s command-alias name=command`).
    SetAlias {
        name: String,
        command: Option<String>,
    },
}
