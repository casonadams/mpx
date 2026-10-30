//! The session actor: sole owner of the workspace, pane handles, exited history, session-wide
//! subscribers, the idempotency cache and snapshot saving. Pane exits arrive on the same mailbox
//! as requests, so a request sent after an exit was observed always sees the updated layout.
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::warn;

use super::idempotency::{Claim, IdemCache};
use super::plan::{self, PaneIdentity};
use super::snapshot::{PaneRecord, build_snapshot};
use super::pipes::Pipes;
use super::stores::Stores;
use crate::node::sleep_until_opt;
use crate::config::Config;
use crate::keys::{TermModes, encode_key_names, wrap_paste};
use crate::layout::{Direction, Workspace};
use crate::node::pacing::Pacing;
use crate::node::pane::{self, InputBuild, Notifier, PaneCtx, PaneHandle, PaneNotice, TermView};
use crate::node::process::basename;
use crate::node::sink::Sink;
use crate::node::{SpawnSpec, now_ms};
use crate::protocol::{
    NewWindowOpts, NodeInfo, PaneId, Request, Response, SessionInfo, SpawnOpts, SplitOpts,
    StreamEvent, VarKind,
};
use crate::store::{SessionSnapshot, SnapshotStore, WindowPlan, plan_restore};
use crate::tmux_shim;

pub(super) type Reply<T> = oneshot::Sender<T>;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// Messages to the session actor.
pub(super) enum SessionMsg {
    /// A request the session answers by itself.
    Req {
        req: Box<Request>,
        reply: Reply<Response>,
    },
    Lookup {
        target: String,
        reply: Reply<Result<PaneHandle, String>>,
    },
    /// Starts a kill. `Some` is a running pane whose exit the caller waits for.
    Kill {
        target: String,
        reply: Reply<Result<Option<oneshot::Receiver<()>>, String>>,
    },
    KillWindow {
        index: Option<usize>,
        reply: Reply<Result<Vec<oneshot::Receiver<()>>, String>>,
    },
    Shutdown {
        reply: Reply<Vec<oneshot::Receiver<()>>>,
    },
    Subscribe {
        filter: Filter,
        sink: Sink,
        reply: Reply<()>,
    },
    SubscribeNode {
        name: String,
        since: Option<u64>,
        sink: Sink,
        reply: Reply<Result<(), String>>,
    },
    IdemBegin {
        key: String,
        reply: Reply<Claim<Response>>,
    },
    IdemFinish {
        key: String,
        value: Box<Response>,
    },
    IdemAbandon {
        key: String,
    },
    Notice(PaneNotice),
}

/// Which session-wide events a subscriber receives.
pub(super) enum Filter {
    Everything,
    /// What a TUI shows: output of the active window's panes, every other pane event, and
    /// `Layout` events when `layout` is set.
    ActiveWindow { layout: bool },
}

impl Filter {
    /// Whether `event` goes to this subscriber; `shown` says if a pane is in the active window.
    fn accepts(&self, event: &StreamEvent, shown: impl Fn(&str) -> bool) -> bool {
        match (self, event) {
            (_, StreamEvent::Layout | StreamEvent::ClientCommand { .. }) => {
                matches!(self, Filter::ActiveWindow { layout: true })
            }
            (Filter::Everything, _) => true,
            (Filter::ActiveWindow { .. }, StreamEvent::Output { name, .. }) => shown(name),
            (Filter::ActiveWindow { .. }, _) => true,
        }
    }
    /// What a subscription asked for: a terminal's view (`active_window`), with or without layout
    /// events, or everything.
    pub(super) fn subscriber(active_window: bool, layout: bool) -> Self {
        if active_window {
            Filter::ActiveWindow { layout }
        } else {
            Filter::Everything
        }
    }

    /// Session-wide: wants events from every pane, so panes must relay them.
    fn session_wide(&self) -> bool {
        matches!(self, Filter::Everything | Filter::ActiveWindow { .. })
    }
}

/// Everything the session needs at startup.
pub(super) struct Boot {
    pub session: String,
    pub endpoint: String,
    pub config: Config,
    pub state_dir: PathBuf,
    pub tx: mpsc::UnboundedSender<SessionMsg>,
    pub ready: oneshot::Sender<()>,
    pub pacing: Arc<Pacing>,
}

pub(super) struct Entry {
    pub handle: PaneHandle,
    /// Spawn options the pane was started with.
    pub opts: SpawnOpts,
    /// The process has ended and the exit was processed.
    pub exited: bool,
}

/// Where a new pane goes in the layout.
pub(super) enum Placement {
    /// Split the focused pane of the active window (new window when empty).
    Spawn,
    /// Split `target` (default: focused pane of the active window).
    Split {
        target: Option<String>,
        dir: Direction,
        percent: Option<u8>,
        focus: bool,
    },
    /// New window. `named` marks a label the caller chose, which automatic naming keeps.
    Window {
        label: String,
        focus: bool,
        named: bool,
    },
}

enum Next {
    Msg(SessionMsg),
    Save,
    Closed,
}

struct Session {
    session: String,
    endpoint: String,
    /// The config as loaded; `config` is this with the session's options on top.
    base_config: Config,
    config: Config,
    created_ms: u64,
    restored_ms: Option<u64>,
    notifier: Notifier,
    workspace: Workspace,
    /// Live panes and exited panes kept on screen (`remain_on_exit`).
    nodes: HashMap<String, Entry>,
    /// Exited panes removed from the layout; oldest first, capped by `history_limit`.
    history: VecDeque<Entry>,
    next_pane_id: PaneId,
    filters: Vec<(Filter, Sink)>,
    /// Shared with every pane: whether output must be relayed here (session-wide subscribers exist).
    relay: Arc<AtomicBool>,
    pacing: Arc<Pacing>,
    /// Named subscribers waiting for a pane of that name: name, replay offset, sink.
    pending: Vec<(String, Option<u64>, Sink)>,
    idem: IdemCache<Response>,
    snapshots: SnapshotStore,
    stores: Stores,
    pipes: Pipes,
    save_due: Option<Instant>,
    shutting_down: bool,
}

/// Runs the session until every handle is gone.
pub(super) async fn run(boot: Boot, mut rx: mpsc::UnboundedReceiver<SessionMsg>) {
    let Boot {
        session,
        endpoint,
        config,
        state_dir,
        tx,
        ready,
        pacing,
    } = boot;
    let notifier: Notifier = Arc::new(move |notice| {
        let _ = tx.send(SessionMsg::Notice(notice));
    });
    let mut session = Session {
        session: session.clone(),
        endpoint,
        base_config: config.clone(),
        config,
        created_ms: now_ms(),
        restored_ms: None,
        notifier,
        workspace: Workspace::new(session),
        nodes: HashMap::new(),
        history: VecDeque::new(),
        next_pane_id: 0,
        filters: Vec::new(),
        relay: Arc::new(AtomicBool::new(false)),
        pacing,
        pending: Vec::new(),
        idem: IdemCache::default(),
        snapshots: SnapshotStore::in_dir(&state_dir),
        stores: Stores::default(),
        pipes: Pipes::default(),
        save_due: None,
        shutting_down: false,
    };
    session.startup_restore();
    let _ = ready.send(());

    loop {
        let next = tokio::select! {
            msg = rx.recv() => msg.map_or(Next::Closed, Next::Msg),
            () = sleep_until_opt(session.save_due) => Next::Save,
        };
        match next {
            Next::Msg(msg) => session.handle(msg).await,
            Next::Save => {
                session.save_due = None;
                session.save(false).await;
            }
            Next::Closed => break,
        }
    }
}

impl Session {
    async fn handle(&mut self, msg: SessionMsg) {
        match msg {
            SessionMsg::Req { req, reply } => {
                let mutates = !is_read_only(&req);
                let before = mutates.then(|| self.workspace.clone());
                let hook = super::hooks::after_request(&req);
                let resp = self.request(*req).await;
                if let Some(hook) = hook.filter(|_| !matches!(resp, Response::Error(_))) {
                    self.fire_hook(hook);
                }
                if mutates {
                    self.schedule_save();
                }
                if before.is_some_and(|before| before != self.workspace) {
                    self.fanout(&StreamEvent::Layout);
                }
                let _ = reply.send(resp);
            }
            SessionMsg::Lookup { target, reply } => {
                let _ = reply.send(self.handle_of(&target));
            }
            SessionMsg::Kill { target, reply } => {
                let result = self.kill(&target);
                self.schedule_save();
                let _ = reply.send(result);
            }
            SessionMsg::KillWindow { index, reply } => {
                let result = self.kill_window(index);
                if result.is_ok() {
                    self.fire_hook("after-kill-window");
                }
                self.schedule_save();
                let _ = reply.send(result);
            }
            SessionMsg::Shutdown { reply } => {
                let stops = self.shutdown().await;
                let _ = reply.send(stops);
            }
            SessionMsg::Subscribe {
                filter,
                sink,
                reply,
            } => {
                let terminal = matches!(filter, Filter::ActiveWindow { layout: true });
                self.filters.push((filter, sink));
                self.sync_relay();
                let _ = reply.send(());
                if terminal {
                    self.fire_hook("client-attached");
                }
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
        }
    }

    fn notice(&mut self, notice: PaneNotice) {
        match notice {
            PaneNotice::Event(event) => {
                if let StreamEvent::Output { name, data, .. } = &event {
                    if self.config.monitor_bell && data.contains('\x07') {
                        self.workspace.mark_bell(name);
                    }
                    if self.config.monitor_activity {
                        self.workspace.mark_activity(name);
                    }
                }
                self.fanout(&event);
            }
            PaneNotice::Exited { name, id, code } => {
                if self.finish(&name, id, code) {
                    self.fanout(&StreamEvent::Exit { name, code });
                }
            }
        }
    }

    /// Runs the command line set for `hook`, if one is set. The receiver resolves when it is done.
    fn fire_hook(&self, hook: &str) -> Option<std::sync::mpsc::Receiver<()>> {
        let line = self.stores.hook(hook)?.clone();
        let target = tmux_shim::EnvTarget {
            endpoint: self.endpoint.clone(),
            session: self.session.clone(),
        };
        Some(super::hooks::fire(target, line))
    }

    /// `config` again: the loaded config with the session's `set-option` values applied.
    fn rebuild_config(&mut self) {
        let mut config = self.base_config.clone();
        for (name, value) in self.stores.vars(VarKind::Option) {
            if let Err(e) = config.apply_option(&name, &value) {
                warn!("option {name}: {e}");
            }
        }
        self.config = config;
        self.sync_relay();
    }

    /// Panes relay output to the session while session-wide subscribers exist, or while activity
    /// or bell monitoring was turned on explicitly (flags then work with no terminal attached).
    /// The defaults alone do not: relaying every chunk of every pane costs a flood about half its
    /// speed, and a terminal that is attached subscribes anyway.
    fn sync_relay(&self) {
        let options = self.stores.vars(VarKind::Option);
        let on = |name: &str| {
            options
                .iter()
                .any(|(k, v)| k == name && crate::config::parse_switch(v) == Ok(true))
        };
        let monitored = on("monitor-activity") || on("monitor-bell");
        let subscribed = self.filters.iter().any(|(filter, _)| filter.session_wide());
        self.relay.store(monitored || subscribed, Ordering::Relaxed);
    }

    /// Terminal subscribers still on the list: those that follow the active window and its layout.
    fn terminal_count(&self) -> usize {
        let terminal = |f: &Filter| matches!(f, Filter::ActiveWindow { layout: true });
        self.filters.iter().filter(|(f, _)| terminal(f)).count()
    }

    fn fanout(&mut self, event: &StreamEvent) {
        let workspace = &self.workspace;
        let shown = |name: &str| workspace.active_window().is_some_and(|w| w.contains(name));
        let before = self.terminal_count();
        self.filters.retain_mut(|(filter, sink)| {
            if filter.accepts(event, shown) {
                sink.push(event.clone())
            } else {
                !sink.is_closed()
            }
        });
        for _ in self.terminal_count()..before {
            self.fire_hook("client-detached");
        }
        self.sync_relay();
    }

    async fn request(&mut self, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong,
            Request::Info => Response::Session(self.session_info()),
            Request::Restore => self.restore(),
            Request::Spawn(opts) => node_reply(self.add_node(opts, Placement::Spawn).await),
            Request::List => Response::NodeList(self.list().await),
            Request::Split(opts) => self.split(opts).await,
            Request::NewWindow(opts) => self.new_window(opts).await,
            Request::Layout { width, height } => self.layout(width, height).await,
            Request::Status { name } => node_reply(self.status(&name).await),
            Request::RenameSession { name } => {
                self.session = name.clone();
                self.workspace.name = name;
                self.schedule_save();
                Response::Ok
            }
            Request::ReloadConfig { path } => {
                match Config::load_at(path.as_deref().map(std::path::Path::new)) {
                    Ok(config) => {
                        self.base_config = config;
                        self.rebuild_config();
                        Response::Ok
                    }
                    Err(e) => Response::Error(e),
                }
            }
            req @ (Request::SetVar { .. }
            | Request::GetVars { .. }
            | Request::SetBuffer { .. }
            | Request::GetBuffer { .. }
            | Request::ListBuffers
            | Request::DeleteBuffer { .. }
            | Request::ClientCommand { .. }
            | Request::JoinPane { .. }
            | Request::SwapWindow { .. }
            | Request::MoveWindow { .. }
            | Request::PipePane { .. }
            | Request::RotateWindow { .. }
            | Request::SetSynchronized { .. }
            | Request::Respawn { .. }) => self.tmux_request(req),
            req => self.target_request(req).await,
        }
    }

    /// Requests behind the tmux surface: variables, paste buffers, client commands, window and pane
    /// moves and pipes.
    fn tmux_request(&mut self, req: Request) -> Response {
        match req {
            Request::SetVar { kind, name, value } => {
                if kind == VarKind::Hook && !super::hooks::HOOK_NAMES.contains(&name.as_str()) {
                    return Response::Error(format!("unknown hook: {name}"));
                }
                if let (VarKind::Option, Some(value)) = (kind, &value) {
                    // Checked on a copy first, so an invalid value is refused and never stored.
                    if let Err(e) = self.base_config.clone().apply_option(&name, value) {
                        return Response::Error(e);
                    }
                }
                self.stores.set_var(kind, name, value);
                if kind == VarKind::Option {
                    self.rebuild_config();
                }
                if matches!(kind, VarKind::Option | VarKind::Binding) {
                    self.fanout(&StreamEvent::ClientCommand {
                        line: "refresh-runtime".to_string(),
                    });
                }
                Response::Ok
            }
            Request::GetVars { kind } => Response::Vars(self.stores.vars(kind)),
            Request::SetBuffer { name, data } => {
                self.stores.push_buffer(name, data);
                Response::Ok
            }
            Request::GetBuffer { name } => match self.stores.buffer(name.as_deref()) {
                Some(buffer) => Response::Buffer(buffer.clone()),
                None => Response::Error(no_buffer(name.as_deref())),
            },
            Request::ListBuffers => Response::Buffers(self.stores.buffers()),
            Request::DeleteBuffer { name } => {
                if self.stores.delete_buffer(name.as_deref()) {
                    Response::Ok
                } else {
                    Response::Error(no_buffer(name.as_deref()))
                }
            }
            Request::ClientCommand { line } => {
                self.fanout(&StreamEvent::ClientCommand { line });
                Response::Ok
            }
            Request::JoinPane {
                src,
                dst,
                side_by_side,
            } => self.join_pane(&src, &dst, side_by_side),
            Request::SwapWindow { a, b } => {
                check(self.workspace.swap_windows(a, b), "No such window")
            }
            Request::MoveWindow { from, to } => {
                check(self.workspace.move_window(from, to), "No such window")
            }
            Request::PipePane {
                name,
                command,
                toggle,
            } => self.pipe_pane(&name, command, toggle),
            Request::RotateWindow { down } => {
                check(self.workspace.rotate_active(down), "No window")
            }
            Request::SetSynchronized { window, on } => {
                let idx = window.unwrap_or(self.workspace.active_idx);
                check(self.workspace.set_synchronized(idx, on), "Window index out of bounds")
            }
            Request::Respawn {
                name,
                kill,
                cwd,
                command,
            } => self.respawn(&name, kill, cwd, command),
            other => Response::Error(format!("not a tmux request: {other:?}")),
        }
    }

    /// Input, screen, sizing and agent requests addressed to one pane.
    async fn target_request(&mut self, req: Request) -> Response {
        match req {
            Request::Send { name, data } => unit(
                self.send(&name, Box::new(move |_: &TermView| data.into_bytes()))
                    .await,
            ),
            Request::SendKeys {
                name,
                keys,
                literal,
            } => unit(
                self.send(
                    &name,
                    Box::new(move |view: &TermView| key_bytes(&keys, literal, view)),
                )
                .await,
            ),
            Request::Paste { name, text, submit } => unit(
                self.send(
                    &name,
                    Box::new(move |view: &TermView| paste_bytes(&text, submit, view)),
                )
                .await,
            ),
            Request::Screen { name } => reply(self.screen(&name).await, Response::Screen),
            Request::ResizePane { name, cols, rows } => unit(self.resize(&name, cols, rows).await),
            Request::GetCwd { name } => reply(self.cwd(&name).await, Response::Cwd),
            req => self.workspace_request(req),
        }
    }

    /// Layout and window operations. Requests that belong to the connection never reach here.
    fn workspace_request(&mut self, req: Request) -> Response {
        match req {
            Request::FocusNext => {
                self.workspace.focus_next();
                Response::Ok
            }
            Request::FocusLast => check(self.workspace.focus_last(), "No previous pane"),
            Request::Focus { name } => match self.target_name(&name) {
                Ok(n) => {
                    let ok = self.workspace.focus(&n);
                    check(ok, &not_found(&n))
                }
                Err(e) => Response::Error(e),
            },
            Request::ResizeSplit {
                name,
                side,
                cells,
                width,
                height,
            } => match self.target_name(&name) {
                Ok(n) => check(
                    self.workspace.resize_split(&n, side, cells, width, height),
                    "No split border to resize",
                ),
                Err(e) => Response::Error(e),
            },
            Request::SwapPane { a, b } => self.swap_pane_req(a, b),
            Request::MarkPane { name, clear } => self.mark_pane_req(name, clear),
            Request::BreakPane { name } => match self.target_name(&name) {
                Ok(n) => check(self.workspace.break_pane(&n), "Pane is alone in its window"),
                Err(e) => Response::Error(e),
            },
            Request::SelectLayout { preset } => {
                check(self.workspace.select_layout(preset), "No window")
            }
            Request::SpreadLayout => check(self.workspace.spread_layout(), "No window"),
            _ => self.window_request(req),
        }
    }

    fn swap_pane_req(&mut self, a: Option<String>, b: Option<String>) -> Response {
        let marked = self.workspace.marked_pane.clone();
        let active = self.workspace.active_window().and_then(|w| w.focused.clone());
        let src = match a {
            Some(s) => match self.target_name(&s) {
                Ok(t) => t,
                Err(e) => return Response::Error(e),
            },
            None => match marked.clone().or_else(|| active.clone()) {
                Some(p) => p,
                None => return Response::Error("No pane to swap".to_string()),
            },
        };
        let dst = match b {
            Some(t) => match self.target_name(&t) {
                Ok(t) => t,
                Err(e) => return Response::Error(e),
            },
            None => match active {
                Some(act) if act != src => act,
                _ => match marked {
                    Some(m) if m != src => m,
                    _ => src.clone(),
                },
            },
        };
        if src == dst {
            Response::Ok
        } else {
            check(self.workspace.swap(&src, &dst), "Cannot swap panes")
        }
    }

    fn mark_pane_req(&mut self, name: Option<String>, clear: bool) -> Response {
        if clear {
            self.workspace.clear_mark_pane();
            return Response::Ok;
        }
        let target = match name {
            Some(n) => match self.target_name(&n) {
                Ok(t) => t,
                Err(e) => return Response::Error(e),
            },
            None => match self.workspace.active_window().and_then(|w| w.focused.clone()) {
                Some(f) => f,
                None => return Response::Error("No active pane to mark".to_string()),
            },
        };
        self.workspace.mark_pane(&target);
        Response::Ok
    }

    /// Window navigation and naming.
    fn window_request(&mut self, req: Request) -> Response {
        match req {
            Request::NextWindow => {
                self.workspace.next_window();
                Response::Ok
            }
            Request::PrevWindow => {
                self.workspace.prev_window();
                Response::Ok
            }
            Request::LastWindow => check(self.workspace.last_window_switch(), "No last window"),
            Request::SelectWindow { index } => check(
                self.workspace.select_window(index),
                "Window index out of bounds",
            ),
            Request::RenameWindow { index, name } => {
                let idx = index.unwrap_or(self.workspace.active_idx);
                check(
                    self.workspace.rename_window(idx, name),
                    "Window index out of bounds",
                )
            }
            _ => Response::Error("Request is handled by the connection".to_string()),
        }
    }

    fn session_info(&self) -> SessionInfo {
        SessionInfo {
            name: self.session.clone(),
            endpoint: self.endpoint.clone(),
            pid: std::process::id(),
            created_ms: self.created_ms,
            windows: self.workspace.windows.len(),
            panes: self.workspace.pane_count(),
            restored_ms: self.restored_ms,
            protocol: crate::protocol::PROTOCOL,
        }
    }

    fn resolve(&self, target: &str) -> Option<String> {
        if let Some(num) = target.strip_prefix('%') {
            let id: PaneId = num.parse().ok()?;
            let live = self.nodes.values().find(|e| e.handle.id() == id);
            let found = live.or_else(|| self.history.iter().rev().find(|e| e.handle.id() == id))?;
            return Some(found.handle.name().to_string());
        }
        let known = self.nodes.contains_key(target)
            || self.history.iter().any(|e| e.handle.name() == target);
        known.then(|| target.to_string())
    }

    fn target_name(&self, target: &str) -> Result<String, String> {
        self.resolve(target).ok_or_else(|| not_found(target))
    }

    fn pipe_pane(&mut self, name: &str, command: Option<String>, toggle: bool) -> Response {
        let pane = match self.target_name(name) {
            Ok(pane) => pane,
            Err(e) => return Response::Error(e),
        };
        let Some(command) = command.filter(|c| !c.trim().is_empty()) else {
            self.pipes.close(&pane);
            return Response::Ok;
        };
        let opened = self
            .handle_of(&pane)
            .and_then(|handle| self.pipes.open(&pane, &handle, &command, toggle));
        match opened {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error(e),
        }
    }

    fn join_pane(&mut self, src: &str, dst: &str, side_by_side: bool) -> Response {
        let (src, dst) = match (self.target_name(src), self.target_name(dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(e), _) | (_, Err(e)) => return Response::Error(e),
        };
        let dir = if side_by_side {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        match self.workspace.join_pane(&src, &dst, dir) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error(e),
        }
    }

    /// Handle of a live or exited pane.
    fn handle_of(&self, target: &str) -> Result<PaneHandle, String> {
        let name = self.target_name(target)?;
        self.nodes
            .get(&name)
            .or_else(|| {
                self.history
                    .iter()
                    .rev()
                    .find(|e| e.handle.name() == name.as_str())
            })
            .map(|e| e.handle.clone())
            .ok_or_else(|| not_found(target))
    }

    async fn info_of(&self, name: &str, full: bool) -> Result<NodeInfo, String> {
        let handle = self.handle_of(name)?;
        let mut info = handle.info(full).await?;
        info.window = self.workspace.window_of(name);
        info.marked = Some(self.workspace.marked_pane.as_deref() == Some(name));
        Ok(info)
    }

    async fn status(&self, name: &str) -> Result<NodeInfo, String> {
        let name = self.target_name(name)?;
        self.info_of(&name, true).await
    }

    async fn list(&self) -> Vec<NodeInfo> {
        let mut infos = Vec::new();
        for entry in self.nodes.values() {
            if let Ok(mut info) = entry.handle.info(true).await {
                info.window = self.workspace.window_of(entry.handle.name());
                info.marked =
                    Some(self.workspace.marked_pane.as_deref() == Some(entry.handle.name()));
                infos.push(info);
            }
        }
        // History holds exited panes that left the layout; a live pane may reuse their name.
        for entry in &self.history {
            if let Ok(mut info) = entry.handle.info(true).await {
                info.marked =
                    Some(self.workspace.marked_pane.as_deref() == Some(entry.handle.name()));
                infos.push(info);
            }
        }
        infos.sort_by_key(|i| i.id);
        infos
    }

    async fn screen(&self, name: &str) -> Result<String, String> {
        self.handle_of(name)?.screen().await
    }

    async fn send(&self, name: &str, build: InputBuild) -> Result<(), String> {
        self.handle_of(name)?.input(build).await
    }

    async fn cwd(&self, name: &str) -> Result<Option<String>, String> {
        self.handle_of(name)?.cwd().await
    }

    async fn resize(&self, name: &str, cols: u16, rows: u16) -> Result<(), String> {
        self.handle_of(name)?.resize(cols, rows).await
    }


    async fn split(&mut self, opts: SplitOpts) -> Response {
        let SplitOpts {
            target,
            spawn,
            direction,
            percent,
            focus,
        } = opts;
        let target = match target.map(|t| self.target_name(&t)).transpose() {
            Ok(target) => target,
            Err(e) => return Response::Error(e),
        };
        let place = Placement::Split {
            target,
            dir: direction,
            percent,
            focus,
        };
        node_reply(self.add_node(spawn, place).await)
    }

    async fn new_window(&mut self, mut opts: NewWindowOpts) -> Response {
        self.resolve_command(&mut opts.spawn);
        let named = opts.name.is_some();
        let label = opts.name.unwrap_or_else(|| basename(&opts.spawn.command));
        let place = Placement::Window {
            label,
            focus: opts.focus,
            named,
        };
        node_reply(self.add_node(opts.spawn, place).await)
    }

    async fn layout(&mut self, width: u16, height: u16) -> Response {
        self.refresh_window_names().await;
        let (rects, dividers) = self.workspace.compute_layout_with_dividers(width, height);
        Response::Layout(rects, dividers, self.workspace.window_tabs())
    }

    async fn refresh_window_names(&mut self) {
        if !self.config.automatic_rename {
            return;
        }
        for idx in 0..self.workspace.windows.len() {
            let focused = self.workspace.windows[idx].focused.clone();
            let handle = focused
                .and_then(|f| self.nodes.get(&f))
                .map(|e| e.handle.clone());
            let label = match handle {
                Some(handle) => handle.label().await.ok().flatten(),
                None => None,
            };
            self.workspace.set_auto_name(idx, label);
        }
    }

    fn check_new_pane(&self, name: &str, place: &Placement) -> Result<(), String> {
        if self.nodes.contains_key(name) {
            return Err(format!("Node '{}' already exists", name));
        }
        match place {
            Placement::Split {
                target: Some(t), ..
            } if self.workspace.window_of(t).is_none() => Err(not_found(t)),
            _ => Ok(()),
        }
    }

    fn placement_anchor(&self, place: &Placement) -> Option<String> {
        let (is_spawn, target) = match place {
            Placement::Spawn => (true, None),
            Placement::Split { target, .. } => (false, target.as_deref()),
            Placement::Window { .. } => (false, None),
        };
        let focused = self
            .workspace
            .active_window()
            .and_then(|w| w.focused.as_deref());
        plan::cwd_anchor(is_spawn, target, focused).map(str::to_string)
    }

    /// Live working directory of a pane, which a new pane inherits.
    async fn anchor_cwd(&self, anchor: Option<String>) -> Option<String> {
        let handle = self.nodes.get(&anchor?)?.handle.clone();
        handle.cwd().await.ok().flatten()
    }

    fn place(&mut self, name: &str, place: Placement) -> Result<(), String> {
        match place {
            Placement::Spawn => {
                self.workspace.add_or_split_with_focus(
                    name.to_string(),
                    Direction::Horizontal,
                    true,
                );
                Ok(())
            }
            Placement::Split {
                target,
                dir,
                percent,
                focus,
            } => {
                self.workspace
                    .split_pane(target.as_deref(), name.to_string(), dir, percent, focus)
            }
            Placement::Window {
                label,
                focus,
                named,
            } => {
                self.workspace
                    .new_window_with_focus(label.clone(), name.to_string(), focus);
                if named {
                    let idx = self.workspace.windows.len() - 1;
                    self.workspace.rename_window(idx, label);
                }
                Ok(())
            }
        }
    }

    /// Starts a pane and places it in the layout.
    async fn add_node(&mut self, mut opts: SpawnOpts, place: Placement) -> Result<NodeInfo, String> {
        self.resolve_command(&mut opts);
        let id = self.next_pane_id;
        let name = plan::spawn_name(&opts.name, &opts.command, id);
        plan::validate_name(&name)?;
        self.check_new_pane(&name, &place)?;

        let anchor = self.placement_anchor(&place);
        let cwd = match opts.cwd.clone() {
            Some(cwd) => Some(cwd),
            None => self.anchor_cwd(anchor).await.or_else(current_dir),
        };
        let handle = self.spawn_pane(&name, id, &opts, cwd)?;
        if let Err(e) = self.place(&name, place) {
            drop(handle.request_kill());
            return Err(e);
        }

        self.next_pane_id += 1;
        self.register(name.clone(), handle, opts);
        self.info_of(&name, true).await
    }

    /// Fills in the program of a pane started without a command: the session's `default-command`
    /// through the default shell, or the default shell itself.
    fn resolve_command(&self, opts: &mut SpawnOpts) {
        if !opts.command.is_empty() {
            return;
        }
        let env_shell = std::env::var("SHELL").ok();
        let (command, args) = plan::default_argv(
            self.config.default_shell.as_deref(),
            env_shell.as_deref(),
            self.config.default_command.as_deref(),
        );
        opts.command = command;
        opts.args = args;
    }

    fn spawn_pane(
        &self,
        name: &str,
        id: PaneId,
        opts: &SpawnOpts,
        cwd: Option<String>,
    ) -> Result<PaneHandle, String> {
        let identity = PaneIdentity {
            endpoint: &self.endpoint,
            session: &self.session,
            name,
            id,
        };
        let tmux = plan::tmux_enabled(opts.tmux_compat, self.config.tmux_compat);
        let env = self.pane_env(&identity, &opts.env, tmux);
        let spec = SpawnSpec {
            name: name.to_string(),
            id,
            command: opts.command.clone(),
            args: opts.args.clone(),
            cwd,
            env,
            term: self.config.default_terminal.clone(),
            scrollback: self.config.scrollback_lines,
        };
        let ctx = PaneCtx {
            notify: Arc::clone(&self.notifier),
            relay: Arc::clone(&self.relay),
            pacing: Arc::clone(&self.pacing),
        };
        pane::spawn(spec, opts.pty, ctx).map_err(|e| e.to_string())
    }

    fn pane_env(
        &self,
        identity: &PaneIdentity,
        extra: &[(String, String)],
        tmux: bool,
    ) -> Vec<(String, String)> {
        let merged: Vec<(String, String)> = self
            .stores
            .environment()
            .into_iter()
            .chain(extra.iter().cloned())
            .collect();
        let extra = merged.as_slice();
        if !tmux {
            return plan::pane_env(identity, extra, None);
        }
        let shim = tmux_shim::shim_dir();
        if let Err(e) = &shim {
            warn!("tmux shim unavailable, PATH not updated: {}", e);
        }
        let inherited = std::env::var("PATH").unwrap_or_default();
        let tmux_env = plan::TmuxEnv {
            daemon_pid: std::process::id(),
            shim_dir: shim.as_deref().map_err(String::as_str),
            inherited_path: &inherited,
        };
        plan::pane_env(identity, extra, Some(&tmux_env))
    }

    /// Attaches a subscriber to a pane, or holds it until a pane of that name is spawned.
    fn subscribe_node(
        &mut self,
        name: String,
        since: Option<u64>,
        sink: Sink,
    ) -> Result<(), String> {
        match self.handle_of(&name) {
            Ok(handle) => {
                handle.attach(since, sink);
                Ok(())
            }
            Err(_) if name.starts_with('%') => Err(not_found(&name)),
            Err(_) => {
                self.pending.push((name, since, sink));
                Ok(())
            }
        }
    }

    /// Attaches subscribers that were waiting for a pane named `name`, replaying from the start so
    /// nothing the pane printed before the attach is lost.
    fn attach_pending(&mut self, name: &str, handle: &PaneHandle) {
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|(_, _, sink)| !sink.is_closed())
            .partition(|(want, _, _)| want.as_str() == name);
        self.pending = waiting;
        for (_, since, sink) in ready {
            handle.attach(Some(since.unwrap_or(0)), sink);
        }
    }

    fn register(&mut self, name: String, handle: PaneHandle, opts: SpawnOpts) {
        self.fanout(&StreamEvent::Spawned {
            name: name.clone(),
            pid: handle.pid(),
        });
        self.attach_pending(&name, &handle);
        self.nodes.insert(
            name,
            Entry {
                handle,
                opts,
                exited: false,
            },
        );
    }

    /// Records an exit: keeps or removes the pane. Ignores stale or repeated notices; returns
    /// whether the exit belonged to the pane's current process.
    fn finish(&mut self, name: &str, id: PaneId, code: Option<i32>) -> bool {
        let Some(entry) = self.nodes.get_mut(name) else {
            return false;
        };
        if entry.handle.id() != id {
            return false;
        }
        entry.exited = true;
        self.fire_hook("pane-exited");
        let Some(entry) = self.nodes.get(name) else {
            return true;
        };
        let keep = entry
            .opts
            .remain_on_exit
            .unwrap_or_else(|| self.config.remain_on_exit.keeps(code));
        if !keep {
            if let Some(entry) = self.nodes.remove(name) {
                self.history.push_back(entry);
            }
            self.workspace.remove(name);
            while self.history.len() > self.config.history_limit {
                self.history.pop_front();
            }
        }
        self.schedule_save();
        true
    }

    /// Restarts a pane's process in its place (tmux `respawn-pane`): the pane keeps its name and
    /// layout slot. An exited pane keeps its id. A running one must be killed first (`kill`), and
    /// its replacement takes a new id, so the old process's exit cannot end the new one.
    fn respawn(
        &mut self,
        target: &str,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Response {
        let name = match self.target_name(target) {
            Ok(name) => name,
            Err(e) => return Response::Error(e),
        };
        let Some(entry) = self.nodes.get(&name) else {
            return Response::Error(format!("can't respawn pane: {name}"));
        };
        let (exited, old_id) = (entry.exited, entry.handle.id());
        if !exited && !kill {
            return Response::Error(format!("pane %{old_id} still active"));
        }
        let mut opts = entry.opts.clone();
        if let Some(argv) = command.filter(|argv| !argv.is_empty()) {
            let mut argv = argv.into_iter();
            opts.command = argv.next().unwrap_or_default();
            opts.args = argv.collect();
        }
        if cwd.is_some() {
            opts.cwd = cwd;
        }
        let id = if exited { old_id } else { self.next_pane_id };
        let spawn_cwd = opts.cwd.clone().or_else(current_dir);
        let handle = match self.spawn_pane(&name, id, &opts, spawn_cwd) {
            Ok(handle) => handle,
            Err(e) => return Response::Error(e),
        };
        if !exited {
            drop(self.nodes[&name].handle.request_kill());
            self.next_pane_id += 1;
        }
        self.register(name, handle, opts);
        Response::Ok
    }

    /// Starts a kill of a live pane (the receiver resolves on exit), or removes an exited one.
    fn kill(&mut self, target: &str) -> Result<Option<oneshot::Receiver<()>>, String> {
        let name = self.target_name(target)?;
        Ok(self.kill_entry(&name))
    }

    fn kill_entry(&mut self, name: &str) -> Option<oneshot::Receiver<()>> {
        if let Some(entry) = self.nodes.get(name) {
            if !entry.exited {
                return Some(entry.handle.request_kill());
            }
            self.nodes.remove(name);
            self.workspace.remove(name);
            return None;
        }
        if let Some(pos) = self.history.iter().rposition(|e| e.handle.name() == name) {
            self.history.remove(pos);
        }
        None
    }

    fn kill_window(&mut self, index: Option<usize>) -> Result<Vec<oneshot::Receiver<()>>, String> {
        let idx = index.unwrap_or(self.workspace.active_idx);
        let panes = self
            .workspace
            .kill_window(idx)
            .ok_or_else(|| "Window index out of bounds".to_string())?;
        Ok(panes.iter().filter_map(|p| self.kill_entry(p)).collect())
    }

    /// Saves a clean snapshot, then starts killing every running pane. Later saves are disabled.
    async fn shutdown(&mut self) -> Vec<oneshot::Receiver<()>> {
        self.save(true).await;
        if let Some(done) = self.fire_hook("session-closed") {
            // A hook that talks back to this session would wait on it, so the wait is short.
            let _ = done.recv_timeout(Duration::from_millis(500));
        }
        self.shutting_down = true;
        self.save_due = None;
        self.nodes
            .values()
            .filter(|e| !e.exited)
            .map(|e| e.handle.request_kill())
            .collect()
    }

    fn restore(&mut self) -> Response {
        if !self.nodes.is_empty() {
            return Response::Error("Session already has panes".to_string());
        }
        match self.snapshots.load() {
            Ok(Some(snapshot)) => {
                self.restore_snapshot(&snapshot);
                Response::Ok
            }
            Ok(None) => Response::Error("No snapshot to restore".to_string()),
            Err(e) => Response::Error(e),
        }
    }

    fn startup_restore(&mut self) {
        if !self.config.restore {
            return;
        }
        match self.snapshots.load() {
            Ok(Some(snapshot)) => self.restore_snapshot(&snapshot),
            Ok(None) => {}
            Err(e) => warn!("could not load session snapshot: {}", e),
        }
    }

    fn restore_snapshot(&mut self, snapshot: &SessionSnapshot) {
        let plan = plan_restore(snapshot, &self.config.resume);
        for window in plan.windows {
            self.restore_window(window);
        }
        if !self.workspace.windows.is_empty() {
            let _ = self.workspace.select_window(plan.active_window);
            self.workspace.last_window = plan.last_window;
        }
        self.restored_ms = Some(now_ms());
        self.schedule_save();
    }

    /// Spawns the window's panes, then installs it with the surviving part of its layout.
    fn restore_window(&mut self, window: WindowPlan) {
        let mut root = Some(window.layout);
        for opts in &window.panes {
            if let Err(e) = self.spawn_restored(opts) {
                warn!("could not restore pane '{}': {}", opts.name, e);
                root = root.and_then(|r| r.remove_leaf(&opts.name));
            }
        }
        let Some(root) = root else { return };
        let label = root.all_leaves().into_iter().next().unwrap_or_default();
        self.workspace
            .install_window(label, window.name, root, window.focused);
    }

    fn spawn_restored(&mut self, opts: &SpawnOpts) -> Result<(), String> {
        plan::validate_name(&opts.name)?;
        if self.nodes.contains_key(&opts.name) {
            return Err(format!("Node '{}' already exists", opts.name));
        }
        let id = self.next_pane_id;
        let handle = self.spawn_pane(&opts.name, id, opts, opts.cwd.clone())?;
        self.next_pane_id += 1;
        self.register(opts.name.clone(), handle, opts.clone());
        Ok(())
    }

    fn schedule_save(&mut self) {
        if !self.shutting_down && self.save_due.is_none() {
            self.save_due = Some(Instant::now() + SAVE_DEBOUNCE);
        }
    }

    async fn save(&mut self, clean: bool) {
        if self.shutting_down && !clean {
            return;
        }
        let snapshot = self.snapshot(clean).await;
        if let Err(e) = self.snapshots.save(&snapshot) {
            warn!("could not save session snapshot: {}", e);
        }
    }

    async fn snapshot(&self, clean: bool) -> SessionSnapshot {
        let mut records = Vec::new();
        for window in &self.workspace.windows {
            for leaf in window.leaves() {
                let Some(entry) = self.nodes.get(&leaf) else {
                    continue;
                };
                let info = entry.handle.info(true).await.ok();
                let cwd = info.as_ref().and_then(|i| i.cwd.clone());
                let fg = info.as_ref().and_then(|i| i.foreground.clone());
                let lookup_name = fg.as_deref().unwrap_or(&entry.opts.command);
                let (resume_rule, session_id) = resolve_resume(
                    lookup_name,
                    &self.config.resume,
                    info.as_ref(),
                    crate::config::home_dir().as_deref(),
                );

                records.push(PaneRecord {
                    name: leaf.clone(),
                    opts: entry.opts.clone(),
                    running: !entry.exited,
                    cwd,
                    resume_rule,
                    session_id,
                });
            }
        }
        build_snapshot(&self.session, now_ms(), clean, &self.workspace, &records)
    }
}

fn resolve_resume(
    lookup_name: &str,
    rules: &[crate::config::ResumeRule],
    info: Option<&NodeInfo>,
    home: Option<&std::path::Path>,
) -> (Option<String>, Option<String>) {
    for rule in rules {
        let matches = rule.name.eq_ignore_ascii_case(lookup_name)
            || rule.detect.iter().any(|d| d.eq_ignore_ascii_case(lookup_name));
        if matches {
            let from_files = rule.id_source.zip(home).and_then(|(source, home)| {
                let pid = info.and_then(|i| i.foreground_pid);
                let tty = info.and_then(|i| i.tty.as_deref());
                crate::store::lookup_session_id(source, home, pid, tty)
            });
            let session_id = from_files
                .or_else(|| rule.env_id.as_ref().and_then(|v| std::env::var(v).ok()));
            return (Some(rule.name.clone()), session_id);
        }
    }
    (None, None)
}

fn is_read_only(req: &Request) -> bool {
    matches!(
        req,
        Request::Ping
            | Request::Info
            | Request::List
            | Request::Logs { .. }
            | Request::Screen { .. }
            | Request::Layout { .. }
            | Request::Status { .. }
            | Request::GetCwd { .. }
            | Request::Send { .. }
            | Request::SendKeys { .. }
            | Request::Paste { .. }
            | Request::ResizePane { .. }
            | Request::SetVar { .. }
            | Request::GetVars { .. }
            | Request::SetBuffer { .. }
            | Request::GetBuffer { .. }
            | Request::ListBuffers
            | Request::DeleteBuffer { .. }
            | Request::ClientCommand { .. }
    )
}

pub(super) fn not_found(name: &str) -> String {
    format!("Node '{}' not found", name)
}

fn no_buffer(name: Option<&str>) -> String {
    match name {
        Some(name) => format!("no buffer {name}"),
        None => "no buffers".to_string(),
    }
}

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

pub(super) fn unit(result: Result<(), String>) -> Response {
    match result {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error(e),
    }
}

pub(super) fn check(ok: bool, error: &str) -> Response {
    if ok {
        Response::Ok
    } else {
        Response::Error(error.to_string())
    }
}

pub(super) fn node_reply(result: Result<NodeInfo, String>) -> Response {
    match result {
        Ok(info) => Response::Node(info),
        Err(e) => Response::Error(e),
    }
}

pub(super) fn reply<T>(result: Result<T, String>, ok: impl FnOnce(T) -> Response) -> Response {
    match result {
        Ok(value) => ok(value),
        Err(e) => Response::Error(e),
    }
}

fn key_bytes(keys: &[String], literal: bool, view: &TermView) -> Vec<u8> {
    if literal {
        keys.concat().into_bytes()
    } else {
        encode_key_names(keys, &view.modes)
    }
}

fn paste_bytes(text: &str, submit: bool, view: &TermView) -> Vec<u8> {
    let modes: TermModes = view.modes;
    let mut bytes = if view.pty {
        wrap_paste(text, &modes)
    } else {
        text.as_bytes().to_vec()
    };
    if submit {
        bytes.push(if view.pty { b'\r' } else { b'\n' });
    }
    bytes
}
