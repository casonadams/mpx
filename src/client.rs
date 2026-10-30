use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, FramedParts, LengthDelimitedCodec, LinesCodec};

use crate::ipc::{IpcStream, connect_stream};
use crate::layout::{Divider, Rect, WindowTab};
use crate::protocol::{
    Buffer, ChannelAction, DaemonMessage, LayoutPreset, NewWindowOpts, NodeInfo, PROTOCOL,
    PopupOpts, Request, Response, SessionInfo, Side, SpawnOpts, SplitOpts, StreamEvent, VarKind,
    WaitCondition, WaitOutcome,
};
use crate::wire;

const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub type LayoutTuple = (Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>);

pub struct Client {
    framed: Framed<IpcStream, LinesCodec>,
    /// Idempotency key applied to the next request only.
    pending_key: Option<String>,
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
        // A daemon from an older mpx does not know the request.
        Response::Error(e) if e.contains("unknown variant") => format!(
            "{op} needs a newer daemon: restart this session (mpx kill-server, then mpx restore)"
        ),
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
        let line = match tokio::time::timeout(HANDSHAKE_TIMEOUT, self.framed.next()).await {
            Ok(Some(Ok(line))) => line,
            Ok(Some(Err(_))) => return Handshake::Incompatible { pid: peer },
            _ => return Handshake::Gone,
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
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, self.call_ok("Shutdown", Request::Shutdown)).await;
        Ok(())
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

    /// Opens a popup. With `opts.wait`, returns once it closes, with its program's status.
    pub async fn popup(&mut self, opts: PopupOpts) -> Result<Option<i32>, String> {
        match self.call(Request::Popup(opts)).await? {
            Response::PopupClosed { status } => Ok(status),
            Response::Ok => Ok(None),
            other => Err(unexpected("Popup", other)),
        }
    }

    /// Closes the open popup (tmux `display-popup -C`).
    pub async fn close_popup(&mut self) -> Result<(), String> {
        self.call_ok("ClosePopup", Request::ClosePopup).await
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
        self.focus_last_with(false).await
    }

    /// `keep_zoom` (tmux `last-pane -Z`) leaves a zoomed window zoomed.
    pub async fn focus_last_with(&mut self, keep_zoom: bool) -> Result<(), String> {
        self.call_ok("FocusLast", Request::FocusLast { keep_zoom })
            .await
    }

    pub async fn focus(&mut self, name: &str) -> Result<(), String> {
        self.focus_with(name, false).await
    }

    /// `keep_zoom` (tmux `select-pane -Z`) leaves a zoomed window zoomed.
    pub async fn focus_with(&mut self, name: &str, keep_zoom: bool) -> Result<(), String> {
        self.call_ok(
            "Focus",
            Request::Focus {
                name: name.to_string(),
                keep_zoom,
            },
        )
        .await
    }

    /// Zooms or unzooms the window of `pane` (the active pane when `None`); `on` None toggles.
    pub async fn zoom(&mut self, pane: Option<&str>, on: Option<bool>) -> Result<(), String> {
        let req = Request::Zoom {
            pane: pane.map(str::to_string),
            on,
        };
        self.call_ok("Zoom", req).await
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
        self.swap_pane_with(Some(a), Some(b), false).await
    }

    pub async fn swap_pane_targets(
        &mut self,
        a: Option<&str>,
        b: Option<&str>,
    ) -> Result<(), String> {
        self.swap_pane_with(a, b, false).await
    }

    /// `keep_zoom` (tmux `swap-pane -Z`) leaves a zoomed window zoomed.
    pub async fn swap_pane_with(
        &mut self,
        a: Option<&str>,
        b: Option<&str>,
        keep_zoom: bool,
    ) -> Result<(), String> {
        let req = Request::SwapPane {
            a: a.map(str::to_string),
            b: b.map(str::to_string),
            keep_zoom,
        };
        self.call_ok("SwapPane", req).await
    }

    pub async fn mark_pane(&mut self, name: Option<&str>, clear: bool) -> Result<(), String> {
        let req = Request::MarkPane {
            name: name.map(str::to_string),
            clear,
        };
        self.call_ok("MarkPane", req).await
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

    pub async fn spread_layout(&mut self) -> Result<(), String> {
        self.call_ok("SpreadLayout", Request::SpreadLayout).await
    }

    pub async fn select_layout(&mut self, preset: Option<LayoutPreset>) -> Result<(), String> {
        self.call_ok("SelectLayout", Request::SelectLayout { preset })
            .await
    }

    /// Rotates the active window's panes through its layout slots (`rotate-window`).
    pub async fn rotate_window(&mut self, down: bool) -> Result<(), String> {
        self.call_ok("RotateWindow", Request::RotateWindow { down })
            .await
    }

    /// Restarts a pane's process in place (`respawn-pane`); see `Request::Respawn`.
    pub async fn respawn(
        &mut self,
        name: &str,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<(), String> {
        let req = Request::Respawn {
            name: name.to_string(),
            kill,
            cwd,
            command,
        };
        self.call_ok("Respawn", req).await
    }

    pub async fn rename_session(&mut self, name: &str) -> Result<(), String> {
        self.call_ok(
            "RenameSession",
            Request::RenameSession {
                name: name.to_string(),
            },
        )
        .await
    }

    /// Makes the daemon re-read its config from `path` (the defaults when `None`).
    pub async fn reload_config(&mut self, path: Option<&std::path::Path>) -> Result<(), String> {
        let path = path.map(|p| p.to_string_lossy().into_owned());
        self.call_ok("ReloadConfig", Request::ReloadConfig { path }).await
    }

    /// Sets (or with `None` removes) a variable of the session's option or environment store.
    pub async fn set_var(
        &mut self,
        kind: VarKind,
        name: &str,
        value: Option<&str>,
    ) -> Result<(), String> {
        let req = Request::SetVar {
            kind,
            name: name.to_string(),
            value: value.map(str::to_string),
        };
        self.call_ok("SetVar", req).await
    }

    /// Turns synchronized input on or off for a window; `None` is the active window.
    pub async fn set_synchronized(&mut self, window: Option<usize>, on: bool) -> Result<(), String> {
        self.call_ok("SetSynchronized", Request::SetSynchronized { window, on }).await
    }

    pub async fn get_vars(&mut self, kind: VarKind) -> Result<Vec<(String, String)>, String> {
        match self.call(Request::GetVars { kind }).await? {
            Response::Vars(vars) => Ok(vars),
            other => Err(unexpected("GetVars", other)),
        }
    }

    /// Records whether pane `name` is in a mode (copy mode) in terminal `terminal`; the daemon's
    /// `pane_in_mode` reads it.
    pub async fn set_pane_mode(&mut self, name: &str, on: bool, terminal: u64) -> Result<(), String> {
        let req = Request::SetPaneMode {
            name: name.to_string(),
            on,
            terminal,
        };
        self.call_ok("SetPaneMode", req).await
    }

    /// The names of the panes that are in a mode.
    pub async fn pane_modes(&mut self) -> Result<Vec<String>, String> {
        match self.call(Request::PaneModes).await? {
            Response::PaneModes(names) => Ok(names),
            other => Err(unexpected("PaneModes", other)),
        }
    }

    /// Pushes a paste buffer; `name` replaces the buffer of that name.
    pub async fn set_buffer(&mut self, name: Option<&str>, data: &str) -> Result<(), String> {
        let req = Request::SetBuffer {
            name: name.map(str::to_string),
            data: data.to_string(),
        };
        self.call_ok("SetBuffer", req).await
    }

    /// The named paste buffer, or the most recent one.
    pub async fn get_buffer(&mut self, name: Option<&str>) -> Result<Buffer, String> {
        let req = Request::GetBuffer {
            name: name.map(str::to_string),
        };
        match self.call(req).await? {
            Response::Buffer(buffer) => Ok(buffer),
            other => Err(unexpected("GetBuffer", other)),
        }
    }

    /// Paste buffers, newest first.
    pub async fn list_buffers(&mut self) -> Result<Vec<Buffer>, String> {
        match self.call(Request::ListBuffers).await? {
            Response::Buffers(buffers) => Ok(buffers),
            other => Err(unexpected("ListBuffers", other)),
        }
    }

    pub async fn delete_buffer(&mut self, name: Option<&str>) -> Result<(), String> {
        let req = Request::DeleteBuffer {
            name: name.map(str::to_string),
        };
        self.call_ok("DeleteBuffer", req).await
    }

    /// tmux `wait-for`: blocks until the action completes on the daemon.
    pub async fn wait_for(&mut self, channel: &str, action: ChannelAction) -> Result<(), String> {
        let req = Request::WaitFor {
            channel: channel.to_string(),
            action,
        };
        self.call_ok("WaitFor", req).await
    }

    /// Has every attached terminal run `line` as a command; returns its sequence number.
    pub async fn client_command(&mut self, line: &str) -> Result<u64, String> {
        let req = Request::ClientCommand {
            line: line.to_string(),
        };
        match self.call(req).await? {
            Response::Seq(seq) => Ok(seq),
            other => Err(unexpected("ClientCommand", other)),
        }
    }

    /// Waits (bounded) until every terminal has applied client command `seq`. With no terminal
    /// attached it returns at once; a terminal that never reports only delays it to the bound.
    pub async fn wait_command(&mut self, seq: u64) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match self.call(Request::CommandSettled { seq }).await? {
                Response::Settled(false) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                Response::Settled(_) => return Ok(()),
                other => return Err(unexpected("CommandSettled", other)),
            }
        }
    }

    /// Reports that this terminal has run client command `seq` and applied its effects.
    pub async fn command_applied(&mut self, seq: u64) -> Result<(), String> {
        self.call_ok("CommandApplied", Request::CommandApplied { seq }).await
    }

    /// Moves pane `src` next to `dst`, splitting it side by side or stacked.
    pub async fn join_pane(
        &mut self,
        src: &str,
        dst: &str,
        side_by_side: bool,
    ) -> Result<(), String> {
        let req = Request::JoinPane {
            src: src.to_string(),
            dst: dst.to_string(),
            side_by_side,
        };
        self.call_ok("JoinPane", req).await
    }

    pub async fn swap_window(&mut self, a: usize, b: usize) -> Result<(), String> {
        self.call_ok("SwapWindow", Request::SwapWindow { a, b }).await
    }

    pub async fn move_window(&mut self, from: usize, to: usize) -> Result<(), String> {
        self.call_ok("MoveWindow", Request::MoveWindow { from, to }).await
    }

    /// Pipes a pane's output into `command`; `None` stops piping. `toggle` stops an existing pipe
    /// instead of replacing it.
    pub async fn pipe_pane(
        &mut self,
        name: &str,
        command: Option<&str>,
        toggle: bool,
    ) -> Result<(), String> {
        let req = Request::PipePane {
            name: name.to_string(),
            command: command.map(str::to_string),
            toggle,
        };
        self.call_ok("PipePane", req).await
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
            layout: false,
        };
        self.stream(req, handler)
            .await
    }

    /// Like `stream_logs`, but hands each event, wrapped by `wrap`, to `tx` and waits while it is
    /// full: a reader that falls behind stops reading, which slows the daemon's delivery instead of
    /// piling events up in memory. Events come as binary frames (`wire`) when the daemon offers
    /// them, as JSON lines otherwise. With `active_window`, pane output comes only for the active
    /// window's panes (a daemon that predates it sends all), and `Layout` events come when windows
    /// or panes are rearranged. Ends when the daemon or `tx`'s receiver goes away.
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
            layout: active_window,
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
        // Built from parts so bytes read along with the daemon's answer are decoded at once,
        // without waiting for more to arrive.
        let mut binary = FramedParts::new::<bytes::Bytes>(parts.io, codec);
        binary.read_buf = parts.read_buf;
        let mut frames = Framed::from_parts(binary);
        while let Some(frame) = frames.next().await {
            let frame = frame.map_err(|e| e.to_string())?;
            // An event this client does not know (from a newer daemon) is skipped, as the JSON
            // stream does; a skipped output chunk shows up as a gap in the offsets.
            let Ok(event) = wire::decode_event(&frame) else {
                continue;
            };
            if tx.send(wrap(event)).await.is_err() {
                break;
            }
        }
        Ok(())
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
