use crate::layout::{Divider, Rect, WindowTab};
use serde::{Deserialize, Serialize};

use super::buffer::{Buffer, BufferRequest, EnvEntry};
use super::node::{NodeInfo, NodeRequest, WaitOutcome};
use super::pane::PaneRequest;
use super::session::{ClientInfo, ClientRequest, ScriptRequest, SessionInfo, SessionRequest};
use super::window::WindowRequest;

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
        /// A terminal attaching: registered as a client of the session, with its size and flags.
        client: Option<ClientInfo>,
        /// With `client`: detach every other client of the session first (`attach -d`).
        #[serde(default)]
        exclusive: bool,
        /// With `exclusive`: send SIGHUP to the parents of the clients detached (`attach -x`). Unix
        /// only; elsewhere the clients are only detached.
        #[serde(default)]
        hangup: bool,
    },
    Node(NodeRequest),
    Pane(PaneRequest),
    Window(WindowRequest),
    Buffer(BufferRequest),
    Session(SessionRequest),
    Script(ScriptRequest),
    Client(ClientRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    Ok,
    /// Subscribed; events follow as binary frames (`wire`) instead of JSON lines.
    BinaryStream,
    Pong,
    Session(SessionInfo),
    /// Boxed: a `NodeInfo` is large, and inline it would size every response by it.
    Node(Box<NodeInfo>),
    NodeList(Vec<NodeInfo>),
    LogChunk {
        data: String,
        next: u64,
    },
    WaitResult(WaitOutcome),
    Error(String),
    Screen(String),
    /// A pane's screen as `Keyframe` answers it: the sequences, the stream offset they are
    /// current to, and the screen's size in columns and rows.
    Keyframe {
        data: String,
        next: u64,
        size: (u16, u16),
    },
    Layout(Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>),
    Cwd(Option<String>),
    Bindings(Vec<(String, String)>),
    Buffer(Buffer),
    Buffers(Vec<Buffer>),
    /// Text captured by `capture-pane`.
    Capture(String),
    /// Session environment entries, sorted by name.
    Environment(Vec<EnvEntry>),
    /// A foreground `run-shell`: the exit status (`None` when killed by a signal) and output.
    Shell {
        status: Option<i32>,
        output: String,
    },
    Text(String),
    Hooks(Vec<(String, String)>),
    Clients(Vec<ClientInfo>),
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
    /// This client was detached: its terminal leaves the session and the stream ends after it.
    Detached,
    /// Run this command line as the client's own command (the prompt's grammar).
    Run {
        line: String,
    },
    /// Redraw the whole client (`refresh-client`).
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonMessage {
    Response(Response),
    Event(StreamEvent),
}
