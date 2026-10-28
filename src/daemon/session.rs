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
use super::pipes::PipesHandle;
use super::plan::{self, PaneIdentity};
use super::snapshot::{PaneRecord, build_snapshot};
use crate::node::sleep_until_opt;
use crate::agents::{self, Harness};
use crate::config::Config;
use crate::keys::{TermModes, encode_key_names, wrap_paste};
use crate::layout::{Direction, Workspace};
use crate::node::pacing::Pacing;
use crate::node::agent::Report;
use crate::node::pane::{self, InputBuild, Notifier, PaneCtx, PaneHandle, PaneNotice, TermView};
use crate::node::process::basename;
use crate::node::sink::Sink;
use crate::node::{SpawnSpec, now_ms};
use crate::protocol::{
    NewWindowOpts, NodeInfo, PaneId, Request, Response, SessionInfo, SpawnOpts, SplitOpts,
    StreamEvent,
};
use crate::store::{AgentSnapshot, SessionSnapshot, SnapshotStore, WindowPlan, plan_restore};
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
    /// What a TUI shows: output of the active window's panes, and every other pane event.
    ActiveWindow,
    Pipe(String),
}

impl Filter {
    /// Whether `event` goes to this subscriber; `shown` says if a pane is in the active window.
    fn accepts(&self, event: &StreamEvent, shown: impl Fn(&str) -> bool) -> bool {
        match (self, event) {
            (Filter::Everything, _) => true,
            (Filter::ActiveWindow, StreamEvent::Output { name, .. }) => shown(name),
            (Filter::ActiveWindow, StreamEvent::Pipe { .. }) => false,
            (Filter::ActiveWindow, _) => true,
            (Filter::Pipe(want), StreamEvent::Pipe { name, .. }) => want == name,
            (Filter::Pipe(_), _) => false,
        }
    }

    /// Session-wide: wants events from every pane, so panes must relay them.
    fn session_wide(&self) -> bool {
        matches!(self, Filter::Everything | Filter::ActiveWindow)
    }
}

/// Everything the session needs at startup.
pub(super) struct Boot {
    pub session: String,
    pub endpoint: String,
    pub config: Config,
    pub state_dir: PathBuf,
    pub tx: mpsc::UnboundedSender<SessionMsg>,
    pub pipes: PipesHandle,
    pub ready: oneshot::Sender<()>,
    pub pacing: Arc<Pacing>,
}

pub(super) struct Entry {
    pub handle: PaneHandle,
    /// Spawn options the pane was started with.
    pub opts: SpawnOpts,
    pub remain: bool,
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
    config: Config,
    created_ms: u64,
    restored_ms: Option<u64>,
    harnesses: Arc<Vec<Harness>>,
    notifier: Notifier,
    workspace: Workspace,
    /// Live panes and exited panes kept on screen (`remain_on_exit`).
    nodes: HashMap<String, Entry>,
    /// Exited panes removed from the layout; oldest first, capped by `history_limit`.
    history: VecDeque<Entry>,
    next_pane_id: PaneId,
    pipes: PipesHandle,
    filters: Vec<(Filter, Sink)>,
    /// Shared with every pane: whether output must be relayed here (session-wide subscribers exist).
    relay: Arc<AtomicBool>,
    pacing: Arc<Pacing>,
    /// Named subscribers waiting for a pane of that name: name, replay offset, sink.
    pending: Vec<(String, Option<u64>, Sink)>,
    idem: IdemCache<Response>,
    snapshots: SnapshotStore,
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
        pipes,
        ready,
        pacing,
    } = boot;
    let notifier: Notifier = Arc::new(move |notice| {
        let _ = tx.send(SessionMsg::Notice(notice));
    });
    let mut session = Session {
        session: session.clone(),
        endpoint,
        config,
        created_ms: now_ms(),
        restored_ms: None,
        harnesses: Arc::new(Vec::new()),
        notifier,
        workspace: Workspace::new(session),
        nodes: HashMap::new(),
        history: VecDeque::new(),
        next_pane_id: 0,
        pipes,
        filters: Vec::new(),
        relay: Arc::new(AtomicBool::new(false)),
        pacing,
        pending: Vec::new(),
        idem: IdemCache::default(),
        snapshots: SnapshotStore::in_dir(&state_dir),
        save_due: None,
        shutting_down: false,
    };
    session.harnesses = Arc::new(agents::registry(&session.config));
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
                let resp = self.request(*req).await;
                if mutates {
                    self.schedule_save();
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
                if filter.session_wide() {
                    self.relay.store(true, Ordering::Relaxed);
                }
                self.filters.push((filter, sink));
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
        }
    }

    fn notice(&mut self, notice: PaneNotice) {
        match notice {
            PaneNotice::Event(event) => self.fanout(&event),
            PaneNotice::Exited { name, id, code } => {
                self.finish(&name, id);
                self.fanout(&StreamEvent::Exit { name, code });
            }
        }
    }

    fn fanout(&mut self, event: &StreamEvent) {
        let workspace = &self.workspace;
        let shown = |name: &str| workspace.active_window().is_some_and(|w| w.contains(name));
        self.filters.retain_mut(|(filter, sink)| {
            if filter.accepts(event, shown) {
                sink.push(event.clone())
            } else {
                !sink.is_closed()
            }
        });
        self.relay.store(
            self.filters.iter().any(|(filter, _)| filter.session_wide()),
            Ordering::Relaxed,
        );
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
            Request::CreatePipe { name, capacity } => {
                self.pipes.create(&name, capacity).await;
                Response::Ok
            }
            Request::WritePipe { name, data } => self.write_pipe(&name, &data).await,
            Request::ListPipes => Response::PipeList(self.pipes.list().await),
            req => self.target_request(req).await,
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
            Request::AgentReport {
                name,
                harness,
                state,
                session_id,
                detail,
                clear,
                task,
                reply,
                edited,
            } => unit(
                self.report(
                    &name,
                    Report {
                        harness,
                        state,
                        session_id,
                        detail,
                        clear,
                        task,
                        reply,
                        edited,
                    },
                )
                .await,
            ),
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
            Request::SwapPane { a, b } => match (self.target_name(&a), self.target_name(&b)) {
                (Ok(a), Ok(b)) => check(self.workspace.swap(&a, &b), "Cannot swap panes"),
                (Err(e), _) | (_, Err(e)) => Response::Error(e),
            },
            Request::BreakPane { name } => match self.target_name(&name) {
                Ok(n) => check(self.workspace.break_pane(&n), "Pane is alone in its window"),
                Err(e) => Response::Error(e),
            },
            Request::SelectLayout { preset } => {
                check(self.workspace.select_layout(preset), "No window")
            }
            _ => self.window_request(req),
        }
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
                infos.push(info);
            }
        }
        // History holds exited panes that left the layout; a live pane may reuse their name.
        for entry in &self.history {
            if let Ok(info) = entry.handle.info(true).await {
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

    async fn report(&self, name: &str, report: Report) -> Result<(), String> {
        self.handle_of(name)?.report(report).await
    }

    async fn write_pipe(&mut self, name: &str, data: &str) -> Response {
        match self.pipes.write(name, data).await {
            Ok(message) => {
                let seq = message.seq;
                self.fanout(&StreamEvent::Pipe {
                    name: name.to_string(),
                    message,
                });
                Response::PipeSeq(seq)
            }
            Err(e) => Response::Error(e),
        }
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

    async fn new_window(&mut self, opts: NewWindowOpts) -> Response {
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
    async fn add_node(&mut self, opts: SpawnOpts, place: Placement) -> Result<NodeInfo, String> {
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
        let remain = plan::remain_on_exit(opts.remain_on_exit, self.config.remain_on_exit);
        self.register(name.clone(), handle, opts, remain);
        self.info_of(&name, true).await
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
        };
        let ctx = PaneCtx {
            notify: Arc::clone(&self.notifier),
            harnesses: Arc::clone(&self.harnesses),
            idle_ms: self.config.agent.idle_ms,
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

    fn register(&mut self, name: String, handle: PaneHandle, opts: SpawnOpts, remain: bool) {
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
                remain,
                exited: false,
            },
        );
    }

    /// Records an exit: keeps or removes the pane. Ignores stale or repeated notices.
    fn finish(&mut self, name: &str, id: PaneId) {
        let Some(entry) = self.nodes.get_mut(name) else {
            return;
        };
        if entry.handle.id() != id {
            return;
        }
        entry.exited = true;
        if !entry.remain {
            if let Some(entry) = self.nodes.remove(name) {
                self.history.push_back(entry);
            }
            self.workspace.remove(name);
            while self.history.len() > self.config.history_limit {
                self.history.pop_front();
            }
        }
        self.schedule_save();
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
            Ok(Some(snapshot)) if !snapshot.clean_shutdown => self.restore_snapshot(&snapshot),
            Ok(_) => {}
            Err(e) => warn!("could not load session snapshot: {}", e),
        }
    }

    fn restore_snapshot(&mut self, snapshot: &SessionSnapshot) {
        #[cfg(unix)]
        let runtime_dir = std::path::Path::new(&self.endpoint)
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(crate::ipc::socket_dir);
        #[cfg(windows)]
        let runtime_dir = crate::ipc::socket_dir();
        let plan = plan_restore(
            snapshot,
            &self.harnesses,
            &runtime_dir,
            self.config.agent.mcp,
        );
        for window in plan.windows {
            self.restore_window(window);
        }
        if !self.workspace.windows.is_empty() {
            let _ = self.workspace.select_window(plan.active_window);
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
        let remain = plan::remain_on_exit(opts.remain_on_exit, self.config.remain_on_exit);
        self.register(opts.name.clone(), handle, opts.clone(), remain);
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
                records.push(PaneRecord {
                    name: leaf.clone(),
                    opts: entry.opts.clone(),
                    running: !entry.exited,
                    cwd: info.as_ref().and_then(|i| i.cwd.clone()),
                    agent: info.and_then(|i| i.agent).map(|a| AgentSnapshot {
                        harness: a.harness,
                        session_id: a.session_id,
                    }),
                });
            }
        }
        build_snapshot(&self.session, now_ms(), clean, &self.workspace, &records)
    }
}

/// Requests that change nothing a snapshot records.
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
            | Request::ListPipes
            | Request::Send { .. }
            | Request::SendKeys { .. }
            | Request::Paste { .. }
            | Request::ResizePane { .. }
            | Request::CreatePipe { .. }
            | Request::WritePipe { .. }
    )
}

pub(super) fn not_found(name: &str) -> String {
    format!("Node '{}' not found", name)
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
