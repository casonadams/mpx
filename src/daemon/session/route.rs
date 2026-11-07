//! Routes actor messages to their handlers. A request is answered here, and the hooks it caused
//! fire here: the layout changed, a window or pane was made, or a client selected one.

use super::answer::{connection_only, reply, unit};
use super::{Origin, Reply, Session, SessionMsg, configure_workspace};
use crate::config::Config;
use crate::node::now_ms;
use crate::node::pane::PaneNotice;
use crate::protocol::{
    BufferRequest, ClientRequest, EnvEntry, NodeRequest, PaneRequest, Request, Response,
    SessionInfo, SessionRequest, StreamEvent, WindowRequest,
};

impl Session {
    pub(super) async fn handle(&mut self, msg: SessionMsg) {
        // Window events follow from what the message can change. A hook's own requests fire none.
        let tracked = matches!(
            msg,
            SessionMsg::Req { .. }
                | SessionMsg::Kill { .. }
                | SessionMsg::KillWindow { .. }
                | SessionMsg::Notice(PaneNotice::Exited { .. })
        );
        let from_hook = matches!(
            msg,
            SessionMsg::Req {
                origin: Origin::Hook,
                ..
            }
        );
        let before = tracked.then(|| self.link_ids());
        self.dispatch(msg).await;
        if let Some(before) = before {
            self.window_events(&before, !from_hook);
        }
    }

    async fn dispatch(&mut self, msg: SessionMsg) {
        match msg {
            SessionMsg::Req { req, origin, reply } => self.serve(*req, origin, reply).await,
            SessionMsg::Lookup { target, reply } => {
                let _ = reply.send(self.handle_of(&target));
            }
            SessionMsg::Kill {
                target,
                origin,
                reply,
            } => self.kill_msg(&target, origin, reply),
            SessionMsg::KillWindow { index, reply } => {
                let result = self.kill_window(index);
                self.schedule_save();
                let _ = reply.send(result);
            }
            SessionMsg::Shutdown { reply } => {
                self.fire("session-closed", None);
                let stops = self.shutdown().await;
                let _ = reply.send(stops);
            }
            SessionMsg::Subscribe { sub, sink, reply } => {
                self.subscribe(sub, sink);
                let _ = reply.send(());
            }
            SessionMsg::SubscribeNode {
                name,
                since,
                sink,
                reply,
            } => {
                let _ = reply.send(self.subscribe_node(name, since, sink));
            }
            SessionMsg::IdemBegin { key, reply } => {
                let claim = self.idem.begin(&key, now_ms());
                let _ = reply.send(claim);
            }
            SessionMsg::IdemFinish { key, value } => self.idem.finish(&key, *value, now_ms()),
            SessionMsg::IdemAbandon { key } => self.idem.abandon(&key),
            SessionMsg::Notice(notice) => self.notice(notice),
            SessionMsg::Hook(msg) => self.hook(msg).await,
        }
    }

    fn notice(&mut self, notice: PaneNotice) {
        match notice {
            PaneNotice::Event(event) => self.pane_event(event),
            PaneNotice::Exited { name, id, code } => self.pane_exited(name, id, code),
        }
    }

    async fn serve(&mut self, req: Request, origin: Origin, reply: Reply<Response>) {
        let mutates = !is_read_only(&req);
        let before = mutates.then(|| self.workspace.clone());
        let made = match req {
            Request::Pane(PaneRequest::Split(_)) => Some("after-split-window"),
            Request::Window(WindowRequest::New(_)) => Some("after-new-window"),
            _ => None,
        };
        let selected = (origin == Origin::Client && selects(&req))
            .then(|| (self.workspace.active_idx, self.active_pane()));
        let resp = self.request(req).await;
        if mutates {
            self.schedule_save();
        }
        if before.is_some_and(|before| before != self.workspace) {
            self.fanout(StreamEvent::Layout);
        }
        if origin == Origin::Client && !matches!(resp, Response::Error(_)) {
            if let (Some(event), Response::Node(info)) = (made, &resp) {
                self.fire(event, Some(info.name.clone()));
            }
            if let Some((window, pane)) = selected {
                self.fire_selection(window, pane);
            }
        }
        let _ = reply.send(resp);
    }

    async fn request(&mut self, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong,
            Request::Info => Response::Session(self.session_info()),
            Request::Restore => self.restore(),
            Request::Node(req) => self.node_request(req).await,
            Request::Pane(req) => self.pane_request(req).await,
            Request::Window(req) => self.window_request(req).await,
            Request::Buffer(req) => self.buffer_request(req).await,
            Request::Session(req) => self.session_request(req).await,
            Request::Script(req) => self.script_request(req).await,
            Request::Client(req) => self.client_request(req),
            Request::Shutdown | Request::Idempotent { .. } | Request::Subscribe { .. } => {
                connection_only()
            }
        }
    }

    /// The session's own state: name, config, key bindings, environment and size.
    async fn session_request(&mut self, req: SessionRequest) -> Response {
        match req {
            SessionRequest::Rename { name } => {
                self.session = name.clone();
                self.workspace.name = name;
                self.schedule_save();
                Response::Ok
            }
            SessionRequest::ReloadConfig { path } => reply(
                Config::load_at(path.as_deref().map(std::path::Path::new)),
                |config| {
                    configure_workspace(&mut self.workspace, &config);
                    self.config = config;
                    self.sync_relay();
                    Response::Ok
                },
            ),
            SessionRequest::SetBinding { name, line } => {
                self.stores.set_binding(name, line);
                self.fanout(StreamEvent::Bindings);
                Response::Ok
            }
            SessionRequest::GetBindings => Response::Bindings(self.stores.bindings()),
            SessionRequest::SetEnvironment { name, change } => {
                unit(self.stores.set_environment(name, change))
            }
            SessionRequest::ShowEnvironment { name } => {
                show_environment(self.stores.environment(), name.as_deref())
            }
            SessionRequest::Layout { width, height } => self.layout(width, height).await,
        }
    }

    fn session_info(&self) -> SessionInfo {
        SessionInfo {
            name: self.session.clone(),
            endpoint: self.endpoint.clone(),
            pid: std::process::id(),
            created_ms: self.created_ms,
            windows: self.workspace.links.len(),
            panes: self.workspace.pane_count(),
            restored_ms: self.restored_ms,
            protocol: crate::protocol::PROTOCOL,
        }
    }
}

fn show_environment(entries: Vec<EnvEntry>, name: Option<&str>) -> Response {
    match name {
        None => Response::Environment(entries),
        Some(name) => match entries.into_iter().find(|e| e.name == name) {
            Some(entry) => Response::Environment(vec![entry]),
            None => Response::Error(format!("unknown variable: {name}")),
        },
    }
}

fn is_read_only(req: &Request) -> bool {
    matches!(
        req,
        Request::Ping
            | Request::Info
            | Request::Node(
                NodeRequest::List
                    | NodeRequest::Logs { .. }
                    | NodeRequest::Screen { .. }
                    | NodeRequest::Keyframe { .. }
                    | NodeRequest::Status { .. }
                    | NodeRequest::Cwd { .. }
                    | NodeRequest::Send { .. }
                    | NodeRequest::SendKeys { .. }
                    | NodeRequest::Paste { .. }
                    | NodeRequest::Resize { .. }
                    | NodeRequest::Capture { .. }
            )
            | Request::Buffer(
                BufferRequest::Set { .. }
                    | BufferRequest::Get { .. }
                    | BufferRequest::List
                    | BufferRequest::Delete { .. }
                    | BufferRequest::Rename { .. }
                    | BufferRequest::Paste(..)
            )
            | Request::Session(
                SessionRequest::SetBinding { .. }
                    | SessionRequest::GetBindings
                    | SessionRequest::ShowEnvironment { .. }
                    | SessionRequest::Layout { .. }
            )
            | Request::Client(
                ClientRequest::List
                    | ClientRequest::Detach { .. }
                    | ClientRequest::Activity { .. }
                    | ClientRequest::Resized { .. }
                    | ClientRequest::Refresh { .. }
                    | ClientRequest::Command { .. }
            )
    )
}

/// Requests that select a window or a pane: they fire the selection hooks when they change it.
fn selects(req: &Request) -> bool {
    matches!(
        req,
        Request::Pane(
            PaneRequest::Focus { .. } | PaneRequest::FocusNext | PaneRequest::FocusLast { .. },
        ) | Request::Window(
            WindowRequest::Next
                | WindowRequest::Prev
                | WindowRequest::Last
                | WindowRequest::Select { .. },
        )
    )
}
