use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, FramedRead, LengthDelimitedCodec, LinesCodec};

use crate::ipc::{IpcStream, connect_stream};
use crate::layout::{Divider, Rect, WindowTab};
use crate::protocol::{
    AgentState, DaemonMessage, LayoutPreset, NewWindowOpts, NodeInfo, PROTOCOL, PipeInfo,
    PipeMessage, Request, Response, SessionInfo, Side, SpawnOpts, SplitOpts, StreamEvent,
    WaitCondition, WaitOutcome,
};
use crate::wire;

pub type LayoutTuple = (Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>);

pub struct Client {
    framed: Framed<IpcStream, LinesCodec>,
    /// Idempotency key applied to the next request only.
    pending_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentReportArgs {
    pub harness: Option<String>,
    pub state: Option<AgentState>,
    pub session_id: Option<String>,
    pub detail: Option<String>,
    pub clear: bool,
    pub task: Option<String>,
    pub reply: Option<String>,
    pub edited: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PipeReadArgs {
    pub since: Option<u64>,
    pub consumer: Option<String>,
    pub wait_ms: Option<u64>,
    pub limit: Option<usize>,
}

/// What answered on a session socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    /// A daemon speaking this client's protocol.
    Compatible,
    /// A daemon this client cannot talk to, typically from another mpx version; `pid` when known.
    Incompatible { pid: Option<u32> },
    /// Nothing answered.
    Gone,
}

fn unexpected(op: &str, resp: Response) -> String {
    match resp {
        Response::Error(e) => e,
        other => format!("Invalid response to {}: {:?}", op, other),
    }
}

impl Client {
    pub async fn connect(endpoint: &str) -> Result<Self, String> {
        let stream = connect_stream(endpoint)
            .await
            .map_err(|e| format!("Failed to connect to daemon at '{}': {}", endpoint, e))?;
        Ok(Self {
            framed: Framed::new(stream, LinesCodec::new()),
            pending_key: None,
        })
    }

    /// Makes the next request idempotent under `key`: the daemon runs it at most once per key and
    /// returns the first response to repeats, so retries are safe.
    pub fn idempotency_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.pending_key = Some(key.into());
        self
    }

    async fn call(&mut self, req: Request) -> Result<Response, String> {
        let req = match self.pending_key.take() {
            Some(key) => Request::Idempotent {
                key,
                request: Box::new(req),
            },
            None => req,
        };
        let req_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.framed.send(req_str).await.map_err(|e| e.to_string())?;
        let line = self
            .framed
            .next()
            .await
            .ok_or_else(|| "Connection closed by daemon".to_string())?
            .map_err(|e| e.to_string())?;
        match serde_json::from_str(&line).map_err(|e| format!("Failed to parse response: {}", e))? {
            DaemonMessage::Response(resp) => Ok(resp),
            DaemonMessage::Event(_) => {
                Err("Unexpected stream event in request-response".to_string())
            }
        }
    }

    async fn call_ok(&mut self, op: &str, req: Request) -> Result<(), String> {
        match self.call(req).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(op, other)),
        }
    }

    async fn call_node(&mut self, op: &str, req: Request) -> Result<NodeInfo, String> {
        match self.call(req).await? {
            Response::Node(info) => Ok(info),
            other => Err(unexpected(op, other)),
        }
    }

    pub async fn ping(&mut self) -> Result<(), String> {
        match self.call(Request::Ping).await? {
            Response::Pong => Ok(()),
            other => Err(unexpected("Ping", other)),
        }
    }

    pub async fn info(&mut self) -> Result<SessionInfo, String> {
        match self.call(Request::Info).await? {
            Response::Session(info) => Ok(info),
            other => Err(unexpected("Info", other)),
        }
    }

    /// Checks that the daemon speaks this client's protocol. Must be the first request.
    pub async fn handshake(&mut self) -> Handshake {
        let peer = self.peer_pid();
        let Ok(req) = serde_json::to_string(&Request::Info) else {
            return Handshake::Gone;
        };
        if self.framed.send(req).await.is_err() {
            return Handshake::Gone;
        }
        let line = match self.framed.next().await {
            None => return Handshake::Gone,
            Some(Err(_)) => return Handshake::Incompatible { pid: peer },
            Some(Ok(line)) => line,
        };
        match serde_json::from_str(&line) {
            Ok(DaemonMessage::Response(Response::Session(info))) if info.protocol == PROTOCOL => {
                Handshake::Compatible
            }
            Ok(DaemonMessage::Response(Response::Session(info))) => Handshake::Incompatible {
                pid: Some(info.pid),
            },
            _ => Handshake::Incompatible { pid: peer },
        }
    }

    #[cfg(unix)]
    fn peer_pid(&self) -> Option<u32> {
        let pid = self.framed.get_ref().peer_cred().ok()?.pid()?;
        u32::try_from(pid).ok()
    }

    #[cfg(not(unix))]
    fn peer_pid(&self) -> Option<u32> {
        None
    }

    pub async fn shutdown(&mut self) -> Result<(), String> {
        self.call_ok("Shutdown", Request::Shutdown).await
    }

    pub async fn spawn(&mut self, opts: SpawnOpts) -> Result<NodeInfo, String> {
        self.call_node("Spawn", Request::Spawn(opts)).await
    }

    pub async fn list(&mut self) -> Result<Vec<NodeInfo>, String> {
        match self.call(Request::List).await? {
            Response::NodeList(list) => Ok(list),
            other => Err(unexpected("List", other)),
        }
    }

    /// Returns `(data, next_offset)`.
    pub async fn logs(
        &mut self,
        name: &str,
        tail: Option<usize>,
        plain: bool,
        since: Option<u64>,
    ) -> Result<(String, u64), String> {
        let req = Request::Logs {
            name: name.to_string(),
            tail,
            plain,
            since,
        };
        match self.call(req).await? {
            Response::LogChunk { data, next } => Ok((data, next)),
            other => Err(unexpected("Logs", other)),
        }
    }

    pub async fn send_input(&mut self, name: &str, data: &str) -> Result<(), String> {
        let req = Request::Send {
            name: name.to_string(),
            data: data.to_string(),
        };
        self.call_ok("Send", req).await
    }

    pub async fn send_keys(
        &mut self,
        name: &str,
        keys: Vec<String>,
        literal: bool,
    ) -> Result<(), String> {
        let req = Request::SendKeys {
            name: name.to_string(),
            keys,
            literal,
        };
        self.call_ok("SendKeys", req).await
    }

    pub async fn paste(&mut self, name: &str, text: &str, submit: bool) -> Result<(), String> {
        let req = Request::Paste {
            name: name.to_string(),
            text: text.to_string(),
            submit,
        };
        self.call_ok("Paste", req).await
    }

    pub async fn kill(&mut self, name: &str) -> Result<(), String> {
        self.call_ok(
            "Kill",
            Request::Kill {
                name: name.to_string(),
            },
        )
        .await
    }

    pub async fn wait(
        &mut self,
        name: &str,
        condition: WaitCondition,
        timeout_ms: Option<u64>,
    ) -> Result<WaitOutcome, String> {
        let req = Request::Wait {
            name: name.to_string(),
            condition,
            timeout_ms,
        };
        match self.call(req).await? {
            Response::WaitResult(outcome) => Ok(outcome),
            other => Err(unexpected("Wait", other)),
        }
    }

    pub async fn screen(&mut self, name: &str) -> Result<String, String> {
        match self
            .call(Request::Screen {
                name: name.to_string(),
            })
            .await?
        {
            Response::Screen(text) => Ok(text),
            other => Err(unexpected("Screen", other)),
        }
    }

    pub async fn split(&mut self, opts: SplitOpts) -> Result<NodeInfo, String> {
        self.call_node("Split", Request::Split(opts)).await
    }

    pub async fn layout(&mut self, width: u16, height: u16) -> Result<LayoutTuple, String> {
        match self.call(Request::Layout { width, height }).await? {
            Response::Layout(rects, dividers, tabs) => Ok((rects, dividers, tabs)),
            other => Err(unexpected("Layout", other)),
        }
    }

    pub async fn new_window(&mut self, opts: NewWindowOpts) -> Result<NodeInfo, String> {
        self.call_node("NewWindow", Request::NewWindow(opts)).await
    }

    pub async fn get_cwd(&mut self, name: &str) -> Result<Option<String>, String> {
        match self
            .call(Request::GetCwd {
                name: name.to_string(),
            })
            .await?
        {
            Response::Cwd(cwd) => Ok(cwd),
            other => Err(unexpected("GetCwd", other)),
        }
    }

    pub async fn next_window(&mut self) -> Result<(), String> {
        self.call_ok("NextWindow", Request::NextWindow).await
    }

    pub async fn prev_window(&mut self) -> Result<(), String> {
        self.call_ok("PrevWindow", Request::PrevWindow).await
    }

    pub async fn last_window(&mut self) -> Result<(), String> {
        self.call_ok("LastWindow", Request::LastWindow).await
    }

    pub async fn select_window(&mut self, index: usize) -> Result<(), String> {
        self.call_ok("SelectWindow", Request::SelectWindow { index })
            .await
    }

    pub async fn rename_window(&mut self, index: Option<usize>, name: &str) -> Result<(), String> {
        let req = Request::RenameWindow {
            index,
            name: name.to_string(),
        };
        self.call_ok("RenameWindow", req).await
    }

    pub async fn kill_window(&mut self, index: Option<usize>) -> Result<(), String> {
        self.call_ok("KillWindow", Request::KillWindow { index })
            .await
    }

    pub async fn next_pane(&mut self) -> Result<(), String> {
        self.call_ok("FocusNext", Request::FocusNext).await
    }

    pub async fn focus_last(&mut self) -> Result<(), String> {
        self.call_ok("FocusLast", Request::FocusLast).await
    }

    pub async fn focus(&mut self, name: &str) -> Result<(), String> {
        self.call_ok(
            "Focus",
            Request::Focus {
                name: name.to_string(),
            },
        )
        .await
    }

    pub async fn status(&mut self, name: &str) -> Result<NodeInfo, String> {
        self.call_node(
            "Status",
            Request::Status {
                name: name.to_string(),
            },
        )
        .await
    }

    pub async fn resize_pane(&mut self, name: &str, cols: u16, rows: u16) -> Result<(), String> {
        let req = Request::ResizePane {
            name: name.to_string(),
            cols,
            rows,
        };
        self.call_ok("ResizePane", req).await
    }

    pub async fn resize_split(
        &mut self,
        name: &str,
        side: Side,
        cells: u16,
        viewport: (u16, u16),
    ) -> Result<(), String> {
        let req = Request::ResizeSplit {
            name: name.to_string(),
            side,
            cells,
            width: viewport.0,
            height: viewport.1,
        };
        self.call_ok("ResizeSplit", req).await
    }

    pub async fn swap_pane(&mut self, a: &str, b: &str) -> Result<(), String> {
        let req = Request::SwapPane {
            a: a.to_string(),
            b: b.to_string(),
        };
        self.call_ok("SwapPane", req).await
    }

    pub async fn break_pane(&mut self, name: &str) -> Result<(), String> {
        self.call_ok(
            "BreakPane",
            Request::BreakPane {
                name: name.to_string(),
            },
        )
        .await
    }

    pub async fn select_layout(&mut self, preset: Option<LayoutPreset>) -> Result<(), String> {
        self.call_ok("SelectLayout", Request::SelectLayout { preset })
            .await
    }

    pub async fn create_pipe(&mut self, name: &str, capacity: Option<usize>) -> Result<(), String> {
        let req = Request::CreatePipe {
            name: name.to_string(),
            capacity,
        };
        self.call_ok("CreatePipe", req).await
    }

    /// Returns the sequence number assigned to the message.
    pub async fn write_pipe(&mut self, name: &str, data: &str) -> Result<u64, String> {
        let req = Request::WritePipe {
            name: name.to_string(),
            data: data.to_string(),
        };
        match self.call(req).await? {
            Response::PipeSeq(seq) => Ok(seq),
            other => Err(unexpected("WritePipe", other)),
        }
    }

    /// Returns `(messages, next_seq)`.
    pub async fn read_pipe(
        &mut self,
        name: &str,
        args: PipeReadArgs,
    ) -> Result<(Vec<PipeMessage>, u64), String> {
        let req = Request::ReadPipe {
            name: name.to_string(),
            since: args.since,
            consumer: args.consumer,
            wait_ms: args.wait_ms,
            limit: args.limit,
        };
        match self.call(req).await? {
            Response::PipeMessages { messages, next } => Ok((messages, next)),
            other => Err(unexpected("ReadPipe", other)),
        }
    }

    pub async fn list_pipes(&mut self) -> Result<Vec<PipeInfo>, String> {
        match self.call(Request::ListPipes).await? {
            Response::PipeList(pipes) => Ok(pipes),
            other => Err(unexpected("ListPipes", other)),
        }
    }

    pub async fn agent_report(&mut self, name: &str, args: AgentReportArgs) -> Result<(), String> {
        let req = Request::AgentReport {
            name: name.to_string(),
            harness: args.harness,
            state: args.state,
            session_id: args.session_id,
            detail: args.detail,
            clear: args.clear,
            task: args.task,
            reply: args.reply,
            edited: args.edited,
        };
        self.call_ok("AgentReport", req).await
    }

    /// Recreates the session from its last snapshot.
    pub async fn restore(&mut self) -> Result<(), String> {
        self.call_ok("Restore", Request::Restore).await
    }

    /// Streams node events; the handler returns false to stop. With `name` and `since`, output from
    /// that byte offset is replayed first without a gap.
    pub async fn stream_logs<F>(
        &mut self,
        name: Option<String>,
        since: Option<u64>,
        handler: F,
    ) -> Result<(), String>
    where
        F: FnMut(StreamEvent) -> bool,
    {
        let req = Request::Subscribe {
            name,
            since,
            binary: false,
            active_window: false,
        };
        self.stream(req, handler)
            .await
    }

    /// Like `stream_logs`, but hands each event, wrapped by `wrap`, to `tx` and waits while it is
    /// full: a reader that falls behind stops reading, which slows the daemon's delivery instead of
    /// piling events up in memory. Events come as binary frames (`wire`) when the daemon offers
    /// them, as JSON lines otherwise. With `active_window`, pane output comes only for the active
    /// window's panes (a daemon that predates it sends all). Ends when the daemon or `tx`'s
    /// receiver goes away.
    pub async fn stream_logs_into<T>(
        mut self,
        (name, since): (Option<String>, Option<u64>),
        active_window: bool,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        let req = Request::Subscribe {
            name,
            since,
            binary: true,
            active_window,
        };
        match self.call(req).await? {
            Response::BinaryStream => return self.forward_frames(tx, wrap).await,
            Response::Ok => {}
            other => return Err(unexpected("Subscribe", other)),
        }
        while let Some(line_res) = self.framed.next().await {
            let line = line_res.map_err(|e| e.to_string())?;
            if let Ok(DaemonMessage::Event(event)) = serde_json::from_str(&line)
                && tx.send(wrap(event)).await.is_err()
            {
                break;
            }
        }
        Ok(())
    }

    /// Reads the rest of the connection as binary frames, starting with any bytes already buffered
    /// after the daemon's answer.
    async fn forward_frames<T>(
        self,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        let parts = self.framed.into_parts();
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(wire::MAX_FRAME)
            .new_codec();
        let mut frames = FramedRead::new(parts.io, codec);
        frames.read_buffer_mut().extend_from_slice(&parts.read_buf);
        while let Some(frame) = frames.next().await {
            let frame = frame.map_err(|e| e.to_string())?;
            let event = wire::decode_event(&frame)?;
            if tx.send(wrap(event)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    /// Streams `StreamEvent::Pipe` messages for one pipe; the handler returns false to stop.
    pub async fn subscribe_pipe<F>(&mut self, name: &str, handler: F) -> Result<(), String>
    where
        F: FnMut(StreamEvent) -> bool,
    {
        self.stream(
            Request::SubscribePipe {
                name: name.to_string(),
            },
            handler,
        )
        .await
    }

    async fn stream<F>(&mut self, req: Request, mut handler: F) -> Result<(), String>
    where
        F: FnMut(StreamEvent) -> bool,
    {
        match self.call(req).await? {
            Response::Ok => {}
            other => return Err(unexpected("Subscribe", other)),
        }
        while let Some(line_res) = self.framed.next().await {
            let line = line_res.map_err(|e| e.to_string())?;
            if let Ok(DaemonMessage::Event(event)) = serde_json::from_str(&line)
                && !handler(event)
            {
                break;
            }
        }
        Ok(())
    }
}
