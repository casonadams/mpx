//! Public face of a pane: the handle that sessions hold and the messages its actor accepts.
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::actor::PaneActor;
use super::pacing::{Inflight, Pacing};
use super::pty::{PtyNode, PtyTerminal};
use super::sink::Sink;
use super::task::TaskNode;
use super::waits::Cond;
use super::{NodeError, Process, SpawnSpec};
use crate::buffer::{RingBuffer, strip_ansi};
use crate::keys::TermModes;
use crate::protocol::{NodeInfo, PaneId, StreamEvent, WaitCondition, WaitOutcome};
use crate::vt::VtScreen;

pub type Reply<T> = oneshot::Sender<T>;

/// Builds input bytes from the pane's current terminal modes.
pub type InputBuild = Box<dyn FnOnce(&TermView) -> Vec<u8> + Send>;

pub struct TermView {
    pub pty: bool,
    pub modes: TermModes,
}

#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    pub tail: Option<usize>,
    /// Render PTY output as plain text.
    pub plain: bool,
    /// Raw bytes from this absolute offset.
    pub since: Option<u64>,
}

/// Text for `query` from a pane's retained output, plus the offset to pass as `since` next time.
/// `screen` is the pane's terminal, when it has one.
pub fn render_logs(
    ring: &RingBuffer,
    screen: Option<&VtScreen>,
    query: &LogQuery,
) -> (String, u64) {
    match query.since {
        Some(offset) => {
            let (bytes, next) = ring.since(offset);
            let text = match screen {
                Some(_) if query.plain => strip_ansi(&bytes),
                _ => String::from_utf8_lossy(&bytes).into_owned(),
            };
            (text, next)
        }
        None => {
            let rendered = screen.filter(|_| query.plain);
            (
                history_text(ring, rendered, query.tail),
                ring.total_written(),
            )
        }
    }
}

fn history_text(ring: &RingBuffer, rendered: Option<&VtScreen>, tail: Option<usize>) -> String {
    match (rendered, tail) {
        (Some(vt), _) => {
            let grid = vt.grid();
            replay_text(&ring.snapshot(), (grid.cols, grid.rows), tail)
        }
        (None, Some(n)) => ring.tail_lines(n),
        (None, None) => ring.snapshot_str(),
    }
}

/// Retained output replayed into a fresh screen of `size` (cols, rows), as plain text with
/// scrollback. The live screen keeps none, so scrolled-off rows cost nothing at output rates;
/// rendering them is paid only when a plain read asks for them.
pub fn replay_text(output: &[u8], size: (usize, usize), tail: Option<usize>) -> String {
    let mut screen = VtScreen::new(size.0, size.1);
    screen.feed(output);
    screen.scrollback_text(tail)
}

/// Messages to a pane actor.
pub enum PaneMsg {
    /// Output bytes from the reader thread.
    Output(Vec<u8>),
    /// The child ended (from the waiter thread).
    Exited(Option<i32>),
    Info {
        full: bool,
        reply: Reply<NodeInfo>,
    },
    Label {
        reply: Reply<Option<String>>,
    },
    Logs {
        query: LogQuery,
        reply: Reply<(String, u64)>,
    },
    Screen {
        reply: Reply<String>,
    },
    Cwd {
        reply: Reply<Option<String>>,
    },
    Input {
        build: InputBuild,
        reply: Reply<Result<(), String>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        reply: Reply<Result<(), String>>,
    },
    Kill {
        reply: Reply<()>,
    },
    Wait {
        cond: Cond,
        timeout: Option<Duration>,
        reply: Reply<WaitOutcome>,
    },
    Subscribe {
        since: Option<u64>,
        sink: Sink,
        reply: Reply<()>,
    },
}

/// What a pane tells its session.
pub enum PaneNotice {
    /// An event for session-wide subscribers.
    Event(StreamEvent),
    /// The process ended. Sent before any waiter for the exit is resolved.
    Exited {
        name: String,
        id: PaneId,
        code: Option<i32>,
    },
}

/// Delivers pane notices to the owning session. Must not block.
pub type Notifier = Arc<dyn Fn(PaneNotice) + Send + Sync>;

/// What a pane needs from its session to start.
pub struct PaneCtx {
    pub notify: Notifier,
    /// Set while session-wide subscribers exist; events are relayed to the session only then.
    pub relay: Arc<AtomicBool>,
    /// Attached terminals' backlogs; the PTY reader waits while one is behind.
    pub pacing: Arc<Pacing>,
}

/// Cheap, cloneable reference to a pane actor.
#[derive(Clone)]
pub struct PaneHandle {
    name: Arc<str>,
    id: PaneId,
    pid: Option<u32>,
    pty: bool,
    tx: mpsc::UnboundedSender<PaneMsg>,
}

const GONE: &str = "Node is unavailable";

impl PaneHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> PaneId {
        self.id
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn pty(&self) -> bool {
        self.pty
    }

    async fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> PaneMsg) -> Result<T, String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(make(tx)).map_err(|_| GONE.to_string())?;
        rx.await.map_err(|_| GONE.to_string())
    }

    pub async fn info(&self, full: bool) -> Result<NodeInfo, String> {
        self.ask(|reply| PaneMsg::Info { full, reply }).await
    }

    /// Automatic window label: the foreground process name.
    pub async fn label(&self) -> Result<Option<String>, String> {
        self.ask(|reply| PaneMsg::Label { reply }).await
    }

    pub async fn logs(&self, query: LogQuery) -> Result<(String, u64), String> {
        self.ask(|reply| PaneMsg::Logs { query, reply }).await
    }

    pub async fn screen(&self) -> Result<String, String> {
        self.ask(|reply| PaneMsg::Screen { reply }).await
    }

    pub async fn cwd(&self) -> Result<Option<String>, String> {
        self.ask(|reply| PaneMsg::Cwd { reply }).await
    }

    pub async fn input(&self, build: InputBuild) -> Result<(), String> {
        self.ask(|reply| PaneMsg::Input { build, reply }).await?
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.ask(|reply| PaneMsg::Resize { cols, rows, reply })
            .await?
    }

    /// Starts a kill. The receiver resolves once the process has exited (immediately if it had).
    pub fn request_kill(&self) -> oneshot::Receiver<()> {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(PaneMsg::Kill { reply });
        rx
    }

    pub async fn wait(
        &self,
        condition: WaitCondition,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome, String> {
        let cond = Cond::from_condition(condition)?;
        self.ask(|reply| PaneMsg::Wait {
            cond,
            timeout,
            reply,
        })
        .await
    }

    /// Registers `sink` for this pane's events. With `since`, output from that offset is queued
    /// first. Ordered with the pane's other messages, so the registration is atomic with output.
    pub fn attach(&self, since: Option<u64>, sink: Sink) {
        let (reply, _ack) = oneshot::channel();
        let _ = self.tx.send(PaneMsg::Subscribe { since, sink, reply });
    }
}

/// Starts a process and its pane actor.
pub fn spawn(spec: SpawnSpec, pty: bool, ctx: PaneCtx) -> Result<PaneHandle, NodeError> {
    let (tx, rx) = mpsc::unbounded_channel();
    let inflight = Arc::new(Inflight::default());
    let (process, terminal): (Box<dyn Process>, Option<PtyTerminal>) = if pty {
        let node = PtyNode::spawn(
            &spec,
            tx.clone(),
            Arc::clone(&ctx.pacing),
            Arc::clone(&inflight),
        )?;
        let terminal = node.terminal();
        (Box::new(node), Some(terminal))
    } else {
        (
            Box::new(TaskNode::spawn(&spec, tx.clone(), Arc::clone(&inflight))?),
            None,
        )
    };
    let handle = PaneHandle {
        name: Arc::from(spec.name.as_str()),
        id: spec.id,
        pid: process.pid(),
        pty,
        tx,
    };
    tokio::spawn(
        PaneActor::new(&spec, process, terminal, ctx)
            .with_inflight(inflight)
            .run(rx),
    );
    Ok(handle)
}
