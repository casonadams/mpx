//! The session actor: sole owner of the workspace, pane handles, exited history, session-wide
//! subscribers, the idempotency cache and snapshot saving. Pane exits arrive on the same mailbox
//! as requests, so a request sent after an exit was observed always sees the updated layout.
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::future::join_all;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::warn;

use super::SESSION_GONE;
use super::channels::Channels;
use super::clients;
use super::hooks::{self, HookTable, Tx};
use super::idempotency::{Claim, IdemCache};
use super::plan::{self, PaneIdentity};
use super::snapshot::{PaneRecord, build_snapshot};
use super::stores::{Stores, no_buffer};
use crate::clients::{Size, WindowSize, effective_size};
use crate::config::Config;
use crate::format::{self, Vars};
use crate::keys::{KeyPolicy, TermModes, encode_key_names, wrap_paste};
use crate::layout::{Direction, RestoredLayout, Workspace, window_not_found};
use crate::node::pacing::Pacing;
use crate::node::pane::{self, InputBuild, Notifier, PaneCtx, PaneHandle, PaneNotice, TermView};
use crate::node::process::basename;
use crate::node::sink::Sink;
use crate::node::sleep_until_opt;
use crate::node::{SpawnSpec, now_ms};
use crate::protocol::{
    BufferRequest, ChannelOp, ClientInfo, ClientRequest, HOOK_EVENTS, LinkOpts, NewWindowOpts,
    NodeInfo, NodeRequest, PaneId, PaneRequest, Request, ResizeOpts, Response, ScriptRequest,
    SessionInfo, SessionRequest, Side, SpawnOpts, SplitOpts, StreamEvent, UnlinkOpts, WindowFit,
    WindowRequest, WindowResize,
};
use crate::protocol::{EnvEntry, PasteBufferOpts};
use crate::store::{SessionSnapshot, SnapshotStore, WindowPlan, plan_restore};
use crate::tui::command::{Command, parse_scoped};
use crate::vt::capture::CaptureSpec;

pub(super) type Reply<T> = oneshot::Sender<T>;

/// Sends `req` to the session as `origin`, answering with the session's response, or with why none
/// came.
pub(super) async fn ask_req(
    tx: &mpsc::UnboundedSender<SessionMsg>,
    origin: Origin,
    req: Request,
) -> Result<Response, String> {
    pane::ask_actor(tx, SESSION_GONE, |reply| SessionMsg::Req {
        req: Box::new(req),
        origin,
        reply,
    })
    .await
}

const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// How often the session drops clients whose connection ended, so the others see the new size.
const CLIENT_CHECK: Duration = Duration::from_secs(1);
/// The largest size `resize-window` gives a window (tmux `WINDOW_MAXIMUM`).
const MAX_WINDOW_SIZE: u16 = 10_000;

/// Where a request came from. A hook's own requests fire no hooks, so a hook cannot loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Client,
    Hook,
}

/// Messages to the session actor.
pub(super) enum SessionMsg {
    /// A request the session answers by itself.
    Req {
        req: Box<Request>,
        origin: Origin,
        reply: Reply<Response>,
    },
    Lookup {
        target: String,
        reply: Reply<Result<PaneHandle, String>>,
    },
    /// Starts a kill. `Some` is a running pane whose exit the caller waits for.
    Kill {
        target: String,
        origin: Origin,
        reply: Reply<Result<Option<oneshot::Receiver<()>>, String>>,
    },
    KillWindow {
        index: Option<usize>,
        reply: Reply<Result<Vec<oneshot::Receiver<()>>, String>>,
    },
    Shutdown {
        reply: Reply<Vec<oneshot::Receiver<()>>>,
    },
    /// Hook and script messages, see `HookMsg`.
    Hook(HookMsg),
    Subscribe {
        sub: Subscription,
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

/// Messages of the hooks and the scripts they run, apart from requests.
pub(super) enum HookMsg {
    /// `wait-for`: answers when the channel allows it; `Wait` and `Lock` may keep waiting.
    Channel {
        channel: String,
        op: ChannelOp,
        reply: Reply<Result<(), String>>,
    },
    /// The program and working directory of a `run-shell` for `target` (the active pane when
    /// `None`).
    ShellEnv {
        target: Option<String>,
        reply: Reply<Result<(String, Option<String>), String>>,
    },
    /// An event of the session that hooks may run on, with the pane it concerns.
    Event {
        event: &'static str,
        pane: Option<String>,
    },
    /// A command line parsed with the session's aliases, with the context it runs in: the
    /// active pane is its target.
    Parse {
        line: String,
        reply: Reply<Result<(Command, hooks::Context), String>>,
    },
    /// The session's windows, its marked pane and the panes of its active window, which the
    /// commands a hook runs take their defaults from.
    State { reply: Reply<hooks::State> },
}

/// A session-wide subscriber: what it sees and, when it is a terminal, how it attaches.
pub(super) struct Subscription {
    pub filter: Filter,
    /// The terminal attaching; None for a subscriber that is not a client.
    pub client: Option<ClientInfo>,
    /// Attaching detaches the clients attached before it (`attach -d`).
    pub exclusive: bool,
    /// With `exclusive`, also hangs up those clients' parents (`attach -x`).
    pub hangup: bool,
}

/// Which session-wide events a subscriber receives.
pub(super) enum Filter {
    Everything,
    /// What a TUI shows: output of the active window's panes, every other pane event, and
    /// `Layout` events when `layout` is set.
    ActiveWindow {
        layout: bool,
    },
}

impl Filter {
    /// Whether `event` goes to this subscriber; `shown` says if a pane is in the active window.
    fn accepts(&self, event: &StreamEvent, shown: impl Fn(&str) -> bool) -> bool {
        match (self, event) {
            (_, StreamEvent::Layout | StreamEvent::Bindings | StreamEvent::Respawned { .. }) => {
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

/// A session-wide subscriber: what it receives, where events go, and the terminal it is, if any.
pub(super) struct Subscriber {
    pub(super) filter: Filter,
    pub(super) sink: Sink,
    pub(super) client: Option<ClientInfo>,
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
    Silence,
    Clients,
    Closed,
}

struct Session {
    session: String,
    endpoint: String,
    config: Config,
    created_ms: u64,
    restored_ms: Option<u64>,
    notifier: Notifier,
    /// The session's own mailbox: hook commands send their requests here.
    tx: Tx,
    hooks: HookTable,
    channels: Channels,
    workspace: Workspace,
    /// The size of the last layout request: what a window with no sized client takes.
    requested: (u16, u16),
    /// Live panes and exited panes kept on screen (`remain_on_exit`).
    nodes: HashMap<String, Entry>,
    /// Exited panes removed from the layout; oldest first, capped by `history_limit`.
    history: VecDeque<Entry>,
    next_pane_id: PaneId,
    filters: Vec<Subscriber>,
    /// Shared with every pane: whether output must be relayed here (session-wide subscribers exist).
    relay: Arc<AtomicBool>,
    pacing: Arc<Pacing>,
    /// Named subscribers waiting for a pane of that name: name, replay offset, sink.
    pending: Vec<(String, Option<u64>, Sink)>,
    idem: IdemCache<Response>,
    snapshots: SnapshotStore,
    stores: Stores,
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
    let hook_tx = tx.clone();
    let notifier: Notifier = Arc::new(move |notice| {
        let _ = tx.send(SessionMsg::Notice(notice));
    });
    let mut workspace = Workspace::new(session.clone());
    workspace.set_renumber_windows(config.window.renumber_windows);
    workspace.set_synchronize_new(config.window.synchronize_panes);
    let mut session = Session {
        session: session.clone(),
        endpoint,
        config,
        created_ms: now_ms(),
        restored_ms: None,
        notifier,
        tx: hook_tx,
        hooks: HookTable::default(),
        channels: Channels::default(),
        workspace,
        requested: (Size::default().width, Size::default().height),
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
        save_due: None,
        shutting_down: false,
    };
    session.start();
    let _ = ready.send(());

    let mut client_check = tokio::time::interval(CLIENT_CHECK);
    loop {
        let next = next_event(&session, &mut rx, &mut client_check).await;
        match next {
            Next::Msg(msg) => session.handle(msg).await,
            Next::Save => {
                session.save_due = None;
                session.save(false).await;
            }
            Next::Silence => session.expire_silence(),
            Next::Clients => session.check_clients(),
            Next::Closed => break,
        }
    }
}

/// Waits for the next message, the save deadline, the silence deadline or the client check.
async fn next_event(
    session: &Session,
    rx: &mut mpsc::UnboundedReceiver<SessionMsg>,
    client_check: &mut tokio::time::Interval,
) -> Next {
    tokio::select! {
        msg = rx.recv() => msg.map_or(Next::Closed, Next::Msg),
        () = sleep_until_opt(session.save_due) => Next::Save,
        () = sleep_until_opt(session.silence_due()) => Next::Silence,
        _ = client_check.tick() => Next::Clients,
    }
}

impl Session {
    /// Restores the last snapshot, then fires `after-new-session` when the session is fresh.
    fn start(&mut self) {
        self.startup_restore();
        if self.restored_ms.is_none() {
            self.fire("after-new-session", None);
        }
    }

    async fn handle(&mut self, msg: SessionMsg) {
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

    /// When the next background window goes quiet, if `monitor-silence` is on.
    fn silence_due(&self) -> Option<tokio::time::Instant> {
        let secs = self.config.window.monitor_silence;
        if secs == 0 {
            return None;
        }
        self.workspace
            .next_silence(secs)
            .map(tokio::time::Instant::from_std)
    }

    /// Flags background windows that have been quiet for `monitor-silence`, and fires
    /// `alert-silence` for each window flagged now.
    fn expire_silence(&mut self) {
        let flagged = self.workspace.expire_silence(
            self.config.window.monitor_silence,
            std::time::Instant::now(),
        );
        if flagged.is_empty() {
            return;
        }
        for number in flagged {
            let pane = self.workspace.focused_at(number);
            self.fire("alert-silence", pane);
        }
        self.fanout(StreamEvent::Layout);
    }

    /// The ids of the session's indices, to tell which were added or removed by a message.
    fn link_ids(&self) -> Vec<usize> {
        self.workspace.links.iter().map(|l| l.id).collect()
    }

    /// Fires `window-linked` for each index added since `before` and `window-unlinked` for each one
    /// removed, unless `fire` is off.
    fn window_events(&self, before: &[usize], fire: bool) {
        if !fire {
            return;
        }
        for (link, window) in self.workspace.linked() {
            if !before.contains(&link.id) {
                self.fire("window-linked", window.focused.clone());
            }
        }
        for id in before {
            if !self.workspace.links.iter().any(|l| l.id == *id) {
                self.fire("window-unlinked", None);
            }
        }
    }

    /// Re-reports the layout when a client that sized a window has gone.
    fn check_clients(&mut self) {
        if clients::gone(&self.filters) {
            self.fanout(StreamEvent::Layout);
        }
    }

    /// Answers a request, then fires the hooks it caused: the layout changed, a window or pane was
    /// made, or a client selected one.
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

    /// Starts a kill for a client or a hook. A client's kill that worked fires `after-kill-pane`.
    fn kill_msg(
        &mut self,
        target: &str,
        origin: Origin,
        reply: Reply<Result<Option<oneshot::Receiver<()>>, String>>,
    ) {
        let killed = self.target_name(target).ok();
        let result = self.kill(target);
        self.schedule_save();
        if let Some(name) = killed
            && origin == Origin::Client
            && result.is_ok()
        {
            self.fire("after-kill-pane", Some(name));
        }
        let _ = reply.send(result);
    }

    /// A message from a hook, or the session event that hooks run on.
    async fn hook(&mut self, msg: HookMsg) {
        match msg {
            HookMsg::Channel { channel, op, reply } => self.channel(&channel, op, reply),
            HookMsg::ShellEnv { target, reply } => {
                let env = self.shell_env(target).await;
                let _ = reply.send(env);
            }
            HookMsg::Event { event, pane } => self.fire(event, pane),
            HookMsg::Parse { line, reply } => {
                let _ = reply.send(self.parse_script(&line));
            }
            HookMsg::State { reply } => {
                let _ = reply.send(hooks::State {
                    windows: self.workspace.window_tabs(),
                    marked: self.workspace.marked_pane().map(str::to_string),
                    panes: self
                        .workspace
                        .active_window()
                        .map(|w| w.leaves())
                        .unwrap_or_default(),
                });
            }
        }
    }

    /// `line` as a command with the session's aliases, and the hook context it runs in: the active
    /// pane is the target the commands act on by default.
    fn parse_script(&self, line: &str) -> Result<(Command, hooks::Context), String> {
        let command = parse_scoped(line, &self.config.alias, &self.session)?;
        let ctx = hooks::Context {
            event: String::new(),
            pane: self.active_pane(),
            base_index: self.config.base_index,
        };
        Ok((command, ctx))
    }

    fn notice(&mut self, notice: PaneNotice) {
        match notice {
            PaneNotice::Event(event) => self.pane_event(event),
            PaneNotice::Exited { name, id, code } => self.pane_exited(name, id, code),
        }
    }

    /// Fires the bell and activity alerts an output event raises, then relays the event.
    fn pane_event(&mut self, event: StreamEvent) {
        if let StreamEvent::Output { name, data, .. } = &event {
            if self.config.window.monitor_bell
                && data.contains('\x07')
                && self.workspace.mark_bell(name)
            {
                self.fire("alert-bell", Some(name.clone()));
            }
            if self.config.window.monitor_activity && self.workspace.mark_activity(name) {
                self.fire("alert-activity", Some(name.clone()));
            }
            self.workspace.note_output(name, std::time::Instant::now());
        }
        self.fanout(event);
    }

    /// A pane's process ended: its exit fires the hooks and reaches the subscribers.
    fn pane_exited(&mut self, name: String, id: PaneId, code: Option<i32>) {
        if self.finish(&name, id, code) {
            self.fire("pane-exited", Some(name.clone()));
            if self.nodes.contains_key(&name) {
                self.fire("pane-died", Some(name.clone()));
            }
            self.fanout(StreamEvent::Exit {
                name: name.clone(),
                code,
            });
        }
    }

    /// Fires `after-select-window` and `after-select-pane` for what a selection changed.
    fn fire_selection(&self, window: usize, pane: Option<String>) {
        if self.workspace.active_idx != window {
            self.fire("after-select-window", self.active_pane());
        }
        if self.active_pane() != pane {
            self.fire("after-select-pane", self.active_pane());
        }
    }

    fn active_pane(&self) -> Option<String> {
        self.workspace
            .active_window()
            .and_then(|w| w.focused.clone())
    }

    /// Runs the commands of hook `event` for `pane`, when any are set.
    fn fire(&self, event: &str, pane: Option<String>) {
        let lines = self.hooks.commands(event);
        if lines.is_empty() {
            return;
        }
        let commands = lines
            .iter()
            .filter_map(
                |line| match parse_scoped(line, &self.config.alias, &self.session) {
                    Ok(command) => Some(command),
                    Err(e) => {
                        warn!("hook {event}: {line}: {e}");
                        None
                    }
                },
            )
            .collect();
        let ctx = hooks::Context {
            event: event.to_string(),
            pane,
            base_index: self.config.base_index,
        };
        hooks::spawn(self.tx.clone(), ctx, commands);
    }

    /// `wait-for`: a signal or unlock answers at once; a wait or lock answers when the channel
    /// lets it through.
    fn channel(&mut self, channel: &str, op: ChannelOp, reply: Reply<Result<(), String>>) {
        let result = match op {
            ChannelOp::Wait => return self.channels.wait(channel, reply),
            ChannelOp::Lock => return self.channels.lock_channel(channel, reply),
            ChannelOp::Signal => {
                self.channels.signal(channel);
                Ok(())
            }
            ChannelOp::Unlock => self.channels.unlock(channel),
        };
        let _ = reply.send(result);
    }

    /// The program `run-shell` runs and the directory it starts in: the target pane's, when it
    /// has one.
    async fn shell_env(&self, target: Option<String>) -> Result<(String, Option<String>), String> {
        let program = self
            .config
            .session
            .default_shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "sh".to_string());
        let cwd = match target.or_else(|| self.active_pane()) {
            Some(target) => self.cwd(&target).await?,
            None => None,
        };
        Ok((program, cwd))
    }

    /// `fmt` with the formats expanded for pane `target` (the active pane when `None`).
    async fn format(
        &self,
        fmt: &str,
        target: Option<String>,
        hook: Option<&str>,
    ) -> Result<String, String> {
        let vars = self.format_vars(target, hook).await?;
        Ok(format::expand(fmt, &vars))
    }

    /// The variables of pane `target` (the active pane when `None`), or of the session alone
    /// when there is no pane. A hook adds its own variables: `hook` and `hook_pane`.
    async fn format_vars(
        &self,
        target: Option<String>,
        hook: Option<&str>,
    ) -> Result<Vars, String> {
        let name = match target {
            Some(target) => Some(self.target_name(&target)?),
            None => self.active_pane(),
        };
        let attached = clients::live(&self.filters).len();
        let times = format::SessionTimes {
            started_ms: self.created_ms,
            restored_ms: self.restored_ms,
        };
        let mut vars = match name {
            Some(name) => {
                let node = self.info_of(&name, true).await?;
                let window = node.window.unwrap_or(0);
                let slot = self
                    .workspace
                    .leaves_at(window)
                    .iter()
                    .position(|leaf| leaf == &name)
                    .unwrap_or(0);
                let tabs = self.workspace.window_tabs();
                let mut view = format::pane_view(&self.session, slot, &node, &[], &tabs);
                view.times = times;
                view.window_base = self.config.base_index;
                view.pane_base = self.config.window.pane_base_index;
                view.session_attached = attached;
                format::pane_vars(&view)
            }
            None => format::session_vars(&self.session, times, attached),
        };
        if let Some(event) = hook {
            let pane_id = vars.get("pane_id").cloned().unwrap_or_default();
            vars.insert("hook", event.to_string());
            vars.insert("hook_pane", pane_id);
        }
        Ok(vars)
    }

    /// `set-hook`: the commands of a hook, after checking that the hook exists and they parse.
    fn set_hook(&mut self, name: &str, command: Option<String>, append: bool) -> Response {
        if !HOOK_EVENTS.contains(&name) {
            return Response::Error(format!("unknown hook '{name}'"));
        }
        if let Some(line) = &command
            && let Err(e) = parse_scoped(line, &self.config.alias, &self.session)
        {
            return Response::Error(e);
        }
        self.hooks.set(name, command, append);
        Response::Ok
    }

    /// `set-hook -R`: runs the commands of a hook now, for the active pane.
    fn run_hook(&self, name: &str) -> Response {
        if !HOOK_EVENTS.contains(&name) {
            return Response::Error(format!("unknown hook '{name}'"));
        }
        self.fire(name, self.active_pane());
        Response::Ok
    }

    /// `set -s command-alias`: sets the alias `name`, or removes it.
    fn set_alias(&mut self, name: String, command: Option<String>) -> Response {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Response::Error(format!("invalid alias name '{name}'"));
        }
        match command {
            Some(command) => self.config.alias.insert(name, command),
            None => self.config.alias.remove(&name),
        };
        Response::Ok
    }

    /// Adds a session-wide subscriber. A terminal (`client`) is registered with this session; with
    /// `exclusive` (`attach -d`) it first detaches the clients attached before it, and with `hangup`
    /// (`attach -x`) it also hangs up their parents. Its arrival or departure can change the window
    /// size the policy picks, so every subscriber lays out again.
    fn subscribe(&mut self, sub: Subscription, sink: Sink) {
        let Subscription {
            filter,
            client,
            exclusive,
            hangup,
        } = sub;
        let client = client.map(|info| ClientInfo {
            session: self.session.clone(),
            activity_ms: now_ms(),
            ..info
        });
        if let (true, Some(info)) = (exclusive, &client) {
            let parents = clients::detach_others(&mut self.filters, &info.name);
            if hangup {
                clients::hang_up(&parents);
            }
        }
        let terminal = client.is_some();
        self.filters.push(Subscriber {
            filter,
            sink,
            client,
        });
        self.sync_relay();
        if terminal {
            self.fanout(StreamEvent::Layout);
        }
    }

    /// The attached clients of this session, and the requests that detach, resize or message them.
    fn client_request(&mut self, req: ClientRequest) -> Response {
        match req {
            ClientRequest::List => Response::Clients(clients::list(&self.filters, &self.session)),
            ClientRequest::Detach { target } => match clients::detach(&mut self.filters, &target) {
                Ok(_) => {
                    self.fanout(StreamEvent::Layout);
                    Response::Ok
                }
                Err(e) => Response::Error(e),
            },
            ClientRequest::Activity { name } => unit(clients::touch(&mut self.filters, &name)),
            ClientRequest::Resized {
                name,
                width,
                height,
            } => match clients::resize(&mut self.filters, &name, width, height) {
                Ok(true) => {
                    self.fanout(StreamEvent::Layout);
                    Response::Ok
                }
                Ok(false) => Response::Ok,
                Err(e) => Response::Error(e),
            },
            ClientRequest::Refresh { name, size } => {
                match clients::refresh(&mut self.filters, &name, size) {
                    Ok(true) => {
                        self.fanout(StreamEvent::Layout);
                        Response::Ok
                    }
                    Ok(false) => Response::Ok,
                    Err(e) => Response::Error(e),
                }
            }
            ClientRequest::Command { name, line } => unit(clients::send(
                &mut self.filters,
                &name,
                StreamEvent::Run { line },
            )),
        }
    }

    /// The size the window is laid out for: the window-size policy over the attached clients, or
    /// `requested` when none is attached.
    fn window_size(&self, requested: (u16, u16)) -> (u16, u16) {
        effective_size(
            self.config.window.window_size,
            self.config.window.default_size,
            &clients::live(&self.filters),
            requested,
        )
    }

    /// Panes relay output to the session only while session-wide subscribers exist: relaying every
    /// chunk of every pane costs a flood about half its speed, and an attached terminal subscribes
    /// anyway.
    fn sync_relay(&self) {
        let subscribed = self.filters.iter().any(|sub| sub.filter.session_wide());
        self.relay.store(subscribed, Ordering::Relaxed);
    }

    /// Sends `event` to every subscriber it reaches, the last of them getting the event itself
    /// rather than a copy; returns how many subscribers accepted it.
    fn fanout(&mut self, event: StreamEvent) -> usize {
        let workspace = &self.workspace;
        let in_active_window = match &event {
            StreamEvent::Output { name, .. } => workspace
                .active_window()
                .is_some_and(|w| w.contains(name)),
            _ => false,
        };
        let reaches = |sub: &Subscriber| sub.filter.accepts(&event, |_| in_active_window);
        let last = self.filters.iter().rposition(reaches);
        let mut pending = Some(event);
        let mut accepted = 0;
        let mut index = 0;
        self.filters.retain_mut(|sub| {
            let at = index;
            index += 1;
            let owned = match &pending {
                Some(event) if sub.filter.accepts(event, |_| in_active_window) => {
                    if Some(at) == last {
                        pending.take()
                    } else {
                        Some(event.clone())
                    }
                }
                _ => None,
            };
            match owned {
                Some(event) => {
                    let kept = sub.sink.push(event);
                    accepted += usize::from(kept);
                    kept
                }
                None => !sub.sink.is_closed(),
            }
        });
        self.sync_relay();
        accepted
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
                Response::Error("Request is handled by the connection".to_string())
            }
        }
    }

    /// Paste buffers.
    async fn buffer_request(&mut self, req: BufferRequest) -> Response {
        match req {
            BufferRequest::Set { name, data, append } => {
                self.stores.set_buffer(name, data, append);
                Response::Ok
            }
            BufferRequest::Get { name } => match self.stores.buffer(name.as_deref()) {
                Some(buffer) => Response::Buffer(buffer.clone()),
                None => Response::Error(no_buffer(name.as_deref())),
            },
            BufferRequest::List => Response::Buffers(self.stores.buffers()),
            BufferRequest::Delete { name } => {
                if self.stores.delete_buffer(name.as_deref()) {
                    Response::Ok
                } else {
                    Response::Error(no_buffer(name.as_deref()))
                }
            }
            BufferRequest::Rename { name, to } => {
                unit(self.stores.rename_buffer(name.as_deref(), to))
            }
            BufferRequest::Paste(opts) => self.paste_buffer(opts).await,
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
            SessionRequest::ReloadConfig { path } => {
                match Config::load_at(path.as_deref().map(std::path::Path::new)) {
                    Ok(config) => {
                        self.workspace
                            .set_renumber_windows(config.window.renumber_windows);
                        self.workspace
                            .set_synchronize_new(config.window.synchronize_panes);
                        self.config = config;
                        self.sync_relay();
                        Response::Ok
                    }
                    Err(e) => Response::Error(e),
                }
            }
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

    /// The script group: formats, hooks and aliases, which the session evaluates itself. The
    /// commands that run a shell or wait on a channel are answered by the connection.
    async fn script_request(&mut self, req: ScriptRequest) -> Response {
        match req {
            ScriptRequest::Format { fmt, target, hook } => reply(
                self.format(&fmt, target, hook.as_deref()).await,
                Response::Text,
            ),
            ScriptRequest::SetHook {
                name,
                command,
                append,
            } => self.set_hook(&name, command, append),
            ScriptRequest::RunHook { name } => self.run_hook(&name),
            ScriptRequest::ShowHooks => Response::Hooks(self.hooks.list()),
            ScriptRequest::SetAlias { name, command } => self.set_alias(name, command),
            ScriptRequest::RunShell { .. }
            | ScriptRequest::RunCommand { .. }
            | ScriptRequest::WaitFor { .. } => {
                Response::Error("Request is handled by the connection".to_string())
            }
        }
    }

    fn join_pane(&mut self, src: &str, dst: &str, side_by_side: bool, detach: bool) -> Response {
        let (src, dst) = match (self.target_name(src), self.target_name(dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(e), _) | (_, Err(e)) => return Response::Error(e),
        };
        let dir = if side_by_side {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        match self.workspace.join_pane(&src, &dst, dir, detach) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error(e),
        }
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
        self.register(name.clone(), handle, opts);
        self.fanout(StreamEvent::Respawned { name });
        Response::Ok
    }

    /// Restarts every pane of window `index` (the active window when `None`), as tmux
    /// `respawn-window`. Without `kill`, a window with a running pane is refused before any of its
    /// panes restarts.
    fn respawn_window(
        &mut self,
        index: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Response {
        let Some(number) = index.or_else(|| self.workspace.active_number()) else {
            return Response::Error("No such window".to_string());
        };
        let panes = self.workspace.leaves_at(number);
        if panes.is_empty() {
            return Response::Error("No such window".to_string());
        }
        let running = panes
            .iter()
            .filter_map(|name| self.nodes.get(name))
            .any(|entry| !entry.exited);
        if running && !kill {
            return Response::Error("window still active".to_string());
        }
        for name in panes {
            if let Response::Error(e) = self.respawn(&name, kill, cwd.clone(), command.clone()) {
                return Response::Error(e);
            }
        }
        Response::Ok
    }

    /// Node requests: spawning, listing, input, screen, sizing, cwd, respawn and status.
    async fn node_request(&mut self, req: NodeRequest) -> Response {
        match req {
            NodeRequest::Spawn(opts) => node_reply(self.add_node(opts, Placement::Spawn).await),
            NodeRequest::List => Response::NodeList(self.list().await),
            NodeRequest::Status { name } => node_reply(self.status(&name).await),
            NodeRequest::Send { name, data } => unit(
                self.send(&name, Box::new(move |_: &TermView| data.into_bytes()))
                    .await,
            ),
            NodeRequest::SendKeys {
                name,
                keys,
                literal,
            } => {
                let policy = KeyPolicy::of(&self.config);
                unit(
                    self.send(
                        &name,
                        Box::new(move |view: &TermView| key_bytes(&keys, literal, view, policy)),
                    )
                    .await,
                )
            }
            NodeRequest::Paste { name, text, submit } => unit(
                self.send(
                    &name,
                    Box::new(move |view: &TermView| paste_bytes(&text, submit, view)),
                )
                .await,
            ),
            NodeRequest::Screen { name } => reply(self.screen(&name).await, Response::Screen),
            NodeRequest::Keyframe { name } => reply(self.keyframe(&name).await, |(data, next, size)| {
                Response::Keyframe { data, next, size }
            }),
            NodeRequest::Capture { name, spec } => {
                reply(self.capture(&name, spec).await, Response::Capture)
            }
            NodeRequest::Resize { name, cols, rows } => unit(self.resize(&name, cols, rows).await),
            NodeRequest::Cwd { name } => reply(self.cwd(&name).await, Response::Cwd),
            NodeRequest::Respawn {
                name,
                kill,
                cwd,
                command,
            } => self.respawn(&name, kill, cwd, command),
            NodeRequest::Logs { .. } | NodeRequest::Kill { .. } | NodeRequest::Wait { .. } => {
                Response::Error("Request is handled by the connection".to_string())
            }
        }
    }

    /// Pane layout: splitting, focus, zoom, marks, joins and layout presets.
    async fn pane_request(&mut self, req: PaneRequest) -> Response {
        match req {
            PaneRequest::Split(opts) => self.split(opts).await,
            PaneRequest::FocusNext => {
                self.workspace.focus_next();
                Response::Ok
            }
            PaneRequest::FocusLast { keep_zoom } => check(
                self.workspace.focus_last_with(keep_zoom),
                "No previous pane",
            ),
            PaneRequest::Focus { name, keep_zoom } => match self.target_name(&name) {
                Ok(n) => {
                    let ok = self.workspace.focus_with(&n, keep_zoom);
                    check(ok, &not_found(&n))
                }
                Err(e) => Response::Error(e),
            },
            PaneRequest::Zoom { pane, on } => match self.zoom(pane, on) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(e),
            },
            PaneRequest::ResizeSplit {
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
            PaneRequest::SwapPane { a, b, keep_zoom } => self.swap_pane_req(a, b, keep_zoom),
            PaneRequest::MarkPane { name, clear } => self.mark_pane_req(name, clear),
            PaneRequest::InputOff { name, off } => self.input_off_req(name, off),
            PaneRequest::BreakPane { name } => match self.target_name(&name) {
                Ok(n) => check(self.workspace.break_pane(&n), "Pane is alone in its window"),
                Err(e) => Response::Error(e),
            },
            PaneRequest::Join {
                src,
                dst,
                side_by_side,
                detach,
            } => self.join_pane(&src, &dst, side_by_side, detach),
            PaneRequest::SelectLayout { preset } => {
                check(self.workspace.select_layout(preset), "No window")
            }
            PaneRequest::SpreadLayout => check(self.workspace.spread_layout(), "No window"),
            PaneRequest::Synchronize { window, on } => self
                .workspace
                .synchronize(window, on)
                .map_or_else(Response::Error, |()| Response::Ok),
        }
    }

    fn swap_pane_req(&mut self, a: Option<String>, b: Option<String>, keep_zoom: bool) -> Response {
        let marked = self.workspace.marked_pane.clone();
        let active = self
            .workspace
            .active_window()
            .and_then(|w| w.focused.clone());
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
            return Response::Ok;
        }
        let swapped = self.workspace.swap(&src, &dst);
        if swapped && !keep_zoom {
            self.workspace.unzoom();
        }
        check(swapped, "Cannot swap panes")
    }

    /// Zooms or unzooms the active window (`on`, else toggled). A `pane` is focused first, which
    /// keeps the zoom; without one the window's focused pane is zoomed.
    fn zoom(&mut self, pane: Option<String>, on: Option<bool>) -> Result<(), String> {
        if let Some(pane) = pane {
            let name = self.target_name(&pane)?;
            self.workspace.focus_with(&name, true);
        }
        if self.workspace.set_zoom(on) {
            Ok(())
        } else {
            Err("No window".to_string())
        }
    }

    /// The named pane, or the active window's focused pane when `name` is None.
    fn pane_or_active(&self, name: Option<String>) -> Result<String, String> {
        match name {
            Some(n) => self.target_name(&n),
            None => self
                .workspace
                .active_window()
                .and_then(|w| w.focused.clone())
                .ok_or_else(|| "No active pane".to_string()),
        }
    }

    /// `select-pane -d` / `-e`: stops or resumes input to the named pane, or the active pane.
    fn input_off_req(&mut self, name: Option<String>, off: bool) -> Response {
        match self.pane_or_active(name) {
            Ok(target) => {
                self.workspace.set_input_off(&target, off);
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }

    fn mark_pane_req(&mut self, name: Option<String>, clear: bool) -> Response {
        if clear {
            self.workspace.clear_mark_pane();
            return Response::Ok;
        }
        let target = match self.pane_or_active(name) {
            Ok(t) => t,
            Err(e) => return Response::Error(e),
        };
        self.workspace.mark_pane(&target);
        Response::Ok
    }

    /// Window navigation, naming, moving and reordering.
    async fn window_request(&mut self, req: WindowRequest) -> Response {
        match req {
            WindowRequest::New(opts) => self.new_window(opts).await,
            WindowRequest::Next => {
                self.workspace.next_window();
                Response::Ok
            }
            WindowRequest::Prev => {
                self.workspace.prev_window();
                Response::Ok
            }
            WindowRequest::Last => check(self.workspace.last_window_switch(), "No last window"),
            WindowRequest::Select { index } => check(
                self.workspace.select_window(index),
                &window_not_found(index),
            ),
            WindowRequest::Rename { index, name } => self.rename_window(index, name),
            WindowRequest::Move { from, to } => self
                .workspace
                .move_window(from, to)
                .map_or_else(Response::Error, |()| Response::Ok),
            WindowRequest::Renumber => {
                self.workspace.compact_numbers();
                Response::Ok
            }
            WindowRequest::Rotate { down, keep_zoom } => {
                check(self.workspace.rotate_active(down, keep_zoom), "No window")
            }
            WindowRequest::Swap { a, b } => self
                .workspace
                .swap_windows(a, b)
                .map_or_else(Response::Error, |()| Response::Ok),
            WindowRequest::Respawn {
                index,
                kill,
                cwd,
                command,
            } => self.respawn_window(index, kill, cwd, command),
            WindowRequest::LinkWindow(req) => self.link_window(req),
            WindowRequest::UnlinkWindow(req) => self.unlink_window(req),
            WindowRequest::ResizeWindow(req) => self.resize_window(req),
            WindowRequest::Kill { .. } => {
                Response::Error("Request is handled by the connection".to_string())
            }
        }
    }

    /// Renames window `index`, or the active window when there is none.
    fn rename_window(&mut self, index: Option<usize>, name: String) -> Response {
        match self.window_index(index) {
            Ok(number) => check(
                self.workspace.rename_window(number, name),
                &window_not_found(number),
            ),
            Err(e) => Response::Error(e),
        }
    }

    /// The index a window request names, or the active one when it names none.
    fn window_index(&self, index: Option<usize>) -> Result<usize, String> {
        index
            .or_else(|| self.workspace.active_number())
            .ok_or_else(|| "No window".to_string())
    }

    /// Answers a window request that may have destroyed windows: their panes are killed, and their
    /// exits are reported as usual.
    fn killed_panes(&mut self, result: Result<Vec<String>, String>) -> Response {
        match result {
            Ok(panes) => {
                for pane in panes {
                    let _ = self.kill_entry(&pane);
                }
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }

    /// `link-window`: shows window `src` at index `dst` as well. The panes of a window the link
    /// destroyed are killed.
    fn link_window(&mut self, req: LinkOpts) -> Response {
        let result = self.window_index(req.src).and_then(|src| {
            self.workspace
                .link_window(src, req.dst, req.place)
                .map(|(_, destroyed)| destroyed)
        });
        self.killed_panes(result)
    }

    /// `unlink-window`: removes one index of a window. The window of an index that is its only one
    /// goes only with `kill`.
    fn unlink_window(&mut self, req: UnlinkOpts) -> Response {
        let result = self
            .window_index(req.index)
            .and_then(|number| self.workspace.unlink_window(number, req.kill));
        self.killed_panes(result)
    }

    /// `resize-window`: pins window `index` (the active one when `None`) to a size of its own,
    /// starting from its current size. `width` and `height` set it, `side` moves that border by
    /// `cells` (1 by default), and `fit` takes the largest or smallest attached client.
    fn resize_window(&mut self, req: ResizeOpts) -> Response {
        let number = match self.window_index(req.index) {
            Ok(number) => number,
            Err(e) => return Response::Error(e),
        };
        let WindowResize {
            width,
            height,
            side,
            cells,
            fit,
        } = req.resize;
        if let Some(bad) = [width, height]
            .into_iter()
            .flatten()
            .find(|v| !(1..=MAX_WINDOW_SIZE).contains(v))
        {
            return Response::Error(format!("size {bad} out of range (1 to {MAX_WINDOW_SIZE})"));
        }
        let (mut w, mut h) = self.window_size_at(number);
        w = width.unwrap_or(w);
        h = height.unwrap_or(h);
        let cells = cells.unwrap_or(1);
        match side {
            Some(Side::Left) => w = shrink(w, cells),
            Some(Side::Right) => w = w.saturating_add(cells),
            Some(Side::Up) => h = shrink(h, cells),
            Some(Side::Down) => h = h.saturating_add(cells),
            None => {}
        }
        if let Some(fit) = fit {
            let policy = match fit {
                WindowFit::Largest => WindowSize::Largest,
                WindowFit::Smallest => WindowSize::Smallest,
            };
            let sized = clients::live(&self.filters);
            (w, h) = effective_size(policy, self.config.window.default_size, &sized, (w, h));
        }
        let size = (w.clamp(1, MAX_WINDOW_SIZE), h.clamp(1, MAX_WINDOW_SIZE));
        match self.workspace.set_manual_size(Some(number), Some(size)) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error(e),
        }
    }

    /// The size window `number` is laid out at: its manual size, else the size the clients give.
    fn window_size_at(&self, number: usize) -> (u16, u16) {
        self.workspace
            .manual_size_at(Some(number))
            .unwrap_or_else(|| self.window_size(self.requested))
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
        info.window = self.workspace.number_of(name);
        info.marked = Some(self.workspace.marked_pane.as_deref() == Some(name));
        info.input_off = Some(self.workspace.input_off(name));
        Ok(info)
    }

    async fn status(&self, name: &str) -> Result<NodeInfo, String> {
        let name = self.target_name(name)?;
        self.info_of(&name, true).await
    }

    /// Every pane's info, asked of all the panes at once. History holds exited panes that left
    /// the layout; a live pane may reuse their name.
    async fn list(&self) -> Vec<NodeInfo> {
        let live = self.nodes.values().map(|entry| (entry, true));
        let exited = self.history.iter().map(|entry| (entry, false));
        let infos = join_all(live.chain(exited).map(|(entry, in_layout)| async move {
            let mut info = entry.handle.info(true).await.ok()?;
            let name = entry.handle.name();
            if in_layout {
                info.window = self.workspace.number_of(name);
                info.input_off = Some(self.workspace.input_off(name));
            }
            info.marked = Some(self.workspace.marked_pane.as_deref() == Some(name));
            Some(info)
        }))
        .await;
        let mut infos: Vec<NodeInfo> = infos.into_iter().flatten().collect();
        infos.sort_by_key(|i| i.id);
        infos
    }

    async fn screen(&self, name: &str) -> Result<String, String> {
        self.handle_of(name)?.screen().await
    }

    async fn keyframe(&self, name: &str) -> Result<(String, u64, (u16, u16)), String> {
        self.handle_of(name)?.keyframe().await
    }

    async fn capture(&self, name: &str, spec: CaptureSpec) -> Result<String, String> {
        self.handle_of(name)?.capture(spec).await
    }

    /// `paste-buffer`: pastes the buffer into the pane; `-d` then deletes it.
    async fn paste_buffer(&mut self, opts: PasteBufferOpts) -> Response {
        let Some(buffer) = self.stores.buffer(opts.name.as_deref()).cloned() else {
            return Response::Error(no_buffer(opts.name.as_deref()));
        };
        let PasteBufferOpts {
            pane,
            delete,
            bracketed,
            raw,
            separator,
            ..
        } = opts;
        let data = buffer.data;
        let build: InputBuild = Box::new(move |view: &TermView| {
            let modes = if view.pty {
                view.modes
            } else {
                TermModes::default()
            };
            crate::keys::paste_buffer_bytes(
                &data,
                separator.as_deref(),
                raw || !view.pty,
                bracketed && view.pty,
                &modes,
            )
        });
        match self.send(&pane, build).await {
            Ok(()) => {
                if delete {
                    self.stores.delete_buffer(Some(&buffer.name));
                }
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }

    async fn send(&self, name: &str, build: InputBuild) -> Result<(), String> {
        if self.workspace.input_off(name) {
            return Ok(());
        }
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
            keep_zoom,
        } = opts;
        let target = match target.map(|t| self.target_name(&t)).transpose() {
            Ok(target) => target,
            Err(e) => return Response::Error(e),
        };
        let zoomed = keep_zoom && self.workspace.zoomed_at(target.as_deref());
        let place = Placement::Split {
            target,
            dir: direction,
            percent,
            focus,
        };
        let result = self.add_node(spawn, place).await;
        if let (true, Ok(info)) = (zoomed, &result) {
            self.workspace.zoom_window_of(&info.name);
        }
        node_reply(result)
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
        self.requested = (width, height);
        let (width, height) = match self.workspace.manual_size_at(None) {
            Some(size) => size,
            None => self.window_size((width, height)),
        };
        let (rects, dividers) = self.workspace.compute_layout_with_dividers(width, height);
        Response::Layout(rects, dividers, self.workspace.window_tabs())
    }

    async fn refresh_window_names(&mut self) {
        if !self.config.window.automatic_rename {
            return;
        }
        let windows: Vec<(usize, Option<String>)> = self
            .workspace
            .linked()
            .map(|(link, w)| (link.number, w.focused.clone()))
            .collect();
        for (number, focused) in windows {
            let handle = focused
                .and_then(|f| self.nodes.get(&f))
                .map(|e| e.handle.clone());
            let label = match handle {
                Some(handle) => handle.label().await.ok().flatten(),
                None => None,
            };
            self.workspace.set_auto_name(number, label);
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
                let number =
                    self.workspace
                        .new_window_with_focus(label.clone(), name.to_string(), focus);
                if named {
                    self.workspace.rename_window(number, label);
                }
                Ok(())
            }
        }
    }

    /// Starts a pane and places it in the layout.
    async fn add_node(
        &mut self,
        mut opts: SpawnOpts,
        place: Placement,
    ) -> Result<NodeInfo, String> {
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
            self.config.session.default_shell.as_deref(),
            env_shell.as_deref(),
            self.config.session.default_command.as_deref(),
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
        let (mut extra, env_remove) = self.stores.spawn_environment();
        extra.extend(opts.env.iter().cloned());
        let env = self.pane_env(&identity, &extra);
        let spec = SpawnSpec {
            name: name.to_string(),
            id,
            command: opts.command.clone(),
            args: opts.args.clone(),
            cwd,
            env,
            env_remove,
            term: self.config.session.default_terminal.clone(),
            scrollback: self.config.session.scrollback_lines,
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
    ) -> Vec<(String, String)> {
        plan::pane_env(identity, extra)
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
        self.fanout(StreamEvent::Spawned {
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
            while self.history.len() > self.config.session.history_limit {
                self.history.pop_front();
            }
        }
        self.schedule_save();
        true
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
        let number = index
            .or_else(|| self.workspace.active_number())
            .ok_or_else(|| "No window".to_string())?;
        let panes = self
            .workspace
            .kill_window(number)
            .ok_or_else(|| window_not_found(number))?;
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
        if !self.config.session.restore {
            return;
        }
        match self.snapshots.load() {
            Ok(Some(snapshot)) => self.restore_snapshot(&snapshot),
            Ok(None) => {}
            Err(e) => warn!("could not load session snapshot: {}", e),
        }
    }

    fn restore_snapshot(&mut self, snapshot: &SessionSnapshot) {
        self.stores.restore_environment(&snapshot.environment);
        let plan = plan_restore(snapshot, &self.config.resume);
        // What each restored index shows, by its position among the restored windows.
        let mut shown: Vec<Option<(usize, usize)>> = Vec::with_capacity(plan.windows.len());
        for window in plan.windows {
            let restored = match window.shares {
                Some(owner) => self.restore_link(owner, &shown, window.number),
                None => self.restore_window(window),
            };
            shown.push(restored);
        }
        if !self.workspace.links.is_empty() {
            let number_of =
                |survivor: usize| shown.get(survivor).copied().flatten().map(|(n, _)| n);
            match number_of(plan.active_window) {
                Some(number) => {
                    if !self.workspace.select_window(number) {
                        warn!("could not select restored window {number}");
                    }
                }
                None => warn!("could not select restored window {}", plan.active_window),
            }
            self.workspace
                .set_last_window(plan.last_window.and_then(number_of));
        }
        self.restored_ms = Some(now_ms());
        self.schedule_save();
    }

    /// Spawns the window's panes, then installs it with the surviving part of its layout. Returns the
    /// installed window's index and window id, or None when no pane of it survived.
    fn restore_window(&mut self, window: WindowPlan) -> Option<(usize, usize)> {
        let mut root = Some(window.layout);
        for opts in &window.panes {
            match self.spawn_restored(opts) {
                Ok(()) => {
                    if let Some(line) = window.startup_input.get(&opts.name) {
                        self.type_into(&opts.name, line);
                    }
                }
                Err(e) => {
                    warn!("could not restore pane '{}': {}", opts.name, e);
                    root = root.and_then(|r| r.remove_leaf(&opts.name));
                }
            }
        }
        let root = root?;
        let label = root.all_leaves().into_iter().next().unwrap_or_default();
        let number = self.workspace.install_window(
            window.number,
            label,
            window.name,
            RestoredLayout {
                root,
                focused: window.focused,
            },
        );
        let id = self.workspace.window_id_at(number)?;
        Some((number, id))
    }

    /// Shows the window that restored index `owner` at index `number` as well. None when the owner
    /// was not restored.
    fn restore_link(
        &mut self,
        owner: usize,
        shown: &[Option<(usize, usize)>],
        number: usize,
    ) -> Option<(usize, usize)> {
        let (_, id) = shown.get(owner).copied().flatten()?;
        Some((self.workspace.link_restored(id, number), id))
    }

    /// Types `line` into pane `name`; the terminal queues it until the shell reads it.
    fn type_into(&self, name: &str, line: &str) {
        let Some(entry) = self.nodes.get(name) else {
            return;
        };
        let handle = entry.handle.clone();
        let bytes = line.as_bytes().to_vec();
        tokio::spawn(async move {
            let _ = handle.input(Box::new(move |_: &TermView| bytes)).await;
        });
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
        let panes: Vec<(String, &Entry)> = self
            .workspace
            .windows
            .iter()
            .flat_map(|window| window.leaves())
            .filter_map(|leaf| self.nodes.get(&leaf).map(|entry| (leaf, entry)))
            .collect();
        let infos = join_all(panes.iter().map(|(_, entry)| entry.handle.info(true))).await;
        let home = crate::config::home_dir();
        let records: Vec<PaneRecord> = panes
            .into_iter()
            .zip(infos)
            .map(|((leaf, entry), info)| {
                let info = info.ok();
                let cwd = info.as_ref().and_then(|i| i.cwd.clone());
                let fg = info.as_ref().and_then(|i| i.foreground.clone());
                let lookup_name = fg.as_deref().unwrap_or(&entry.opts.command);
                let (resume_rule, session_id) = resolve_resume(
                    lookup_name,
                    &self.config.resume,
                    info.as_ref(),
                    home.as_deref(),
                );
                PaneRecord {
                    name: leaf,
                    opts: entry.opts.clone(),
                    running: !entry.exited,
                    cwd,
                    resume_rule,
                    session_id,
                }
            })
            .collect();
        let mut snapshot =
            build_snapshot(&self.session, now_ms(), clean, &self.workspace, &records);
        snapshot.environment = self.stores.environment();
        snapshot
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
            || rule
                .detect
                .iter()
                .any(|d| d.eq_ignore_ascii_case(lookup_name));
        if matches {
            let from_files = rule.id_source.zip(home).and_then(|(source, home)| {
                let pid = info.and_then(|i| i.foreground_pid);
                let tty = info.and_then(|i| i.tty.as_deref());
                crate::store::lookup_session_id(source, home, pid, tty)
            });
            let session_id =
                from_files.or_else(|| rule.env_id.as_ref().and_then(|v| std::env::var(v).ok()));
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

pub(super) fn not_found(name: &str) -> String {
    format!("Node '{}' not found", name)
}

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// `v` less `by` cells, or `v` itself when it is smaller than that (tmux `resize-window -L`).
fn shrink(v: u16, by: u16) -> u16 {
    if v >= by { v - by } else { v }
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
        Ok(info) => Response::Node(Box::new(info)),
        Err(e) => Response::Error(e),
    }
}

pub(super) fn reply<T>(result: Result<T, String>, ok: impl FnOnce(T) -> Response) -> Response {
    match result {
        Ok(value) => ok(value),
        Err(e) => Response::Error(e),
    }
}

fn key_bytes(keys: &[String], literal: bool, view: &TermView, policy: KeyPolicy) -> Vec<u8> {
    if literal {
        keys.concat().into_bytes()
    } else {
        encode_key_names(keys, &view.modes, policy)
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

fn show_environment(entries: Vec<EnvEntry>, name: Option<&str>) -> Response {
    match name {
        None => Response::Environment(entries),
        Some(name) => match entries.into_iter().find(|e| e.name == name) {
            Some(entry) => Response::Environment(vec![entry]),
            None => Response::Error(format!("unknown variable: {name}")),
        },
    }
}
