//! The session actor: sole owner of the workspace, pane handles, exited history, session-wide
//! subscribers, the idempotency cache and snapshot saving. Pane exits arrive on the same mailbox
//! as requests, so a request sent after an exit was observed always sees the updated layout.
//!
//! This file holds the types the actor shares and its run loop. Each submodule owns one
//! responsibility of the actor's `impl Session`.

mod answer;
mod buffers;
mod events;
mod lifecycle;
mod node;
mod panes;
mod persist;
mod registry;
mod restore;
mod route;
mod script;
mod spawn;
mod subscribers;
mod windows;

pub(super) use answer::reply;
pub(super) use subscribers::{Filter, Subscriber, Subscription};

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::SESSION_GONE;
use super::channels::Channels;
use super::hooks::{Context, HookTable, State, Tx};
use super::idempotency::{Claim, IdemCache};
use super::stores::Stores;
use crate::clients::Size;
use crate::config::Config;
use crate::layout::{Direction, Workspace};
use crate::node::now_ms;
use crate::node::pacing::Pacing;
use crate::node::pane::{self, Notifier, PaneHandle, PaneNotice};
use crate::node::sink::Sink;
use crate::node::sleep_until_opt;
use crate::protocol::{ChannelOp, PaneId, Request, Response, SpawnOpts};
use crate::store::SnapshotStore;
use crate::tui::command::Command;

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

/// How often the session drops clients whose connection ended, so the others see the new size.
const CLIENT_CHECK: Duration = Duration::from_secs(1);

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
        reply: Reply<Result<(Command, Context), String>>,
    },
    /// The session's windows, its marked pane and the panes of its active window, which the
    /// commands a hook runs take their defaults from.
    State { reply: Reply<State> },
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

struct Entry {
    handle: PaneHandle,
    /// Spawn options the pane was started with.
    opts: SpawnOpts,
    /// The process has ended and the exit was processed.
    exited: bool,
}

/// Where a new pane goes in the layout.
enum Placement {
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

fn configure_workspace(workspace: &mut Workspace, config: &Config) {
    workspace.set_renumber_windows(config.window.renumber_windows);
    workspace.set_synchronize_new(config.window.synchronize_panes);
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
    configure_workspace(&mut workspace, &config);
    let mut session = Session {
        session,
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
}
