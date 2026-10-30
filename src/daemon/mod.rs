mod dispatch;
pub mod idempotency;
pub mod plan;
mod session;
pub mod snapshot;

use futures_util::{SinkExt, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::ipc::{AsyncStream, IpcListener};
use crate::node::pane::PaneHandle;
use crate::node::sink::Sink;
use crate::node::now_ms;
use crate::node::pacing::{Backlog, CloseOnDrop, Pacing};
use crate::protocol::{DaemonMessage, Request, Response, StreamEvent};
use crate::wire;
use idempotency::Claim;
use session::{Filter, SessionMsg};

pub(super) const KILL_GRACE: Duration = Duration::from_secs(2);
const SESSION_GONE: &str = "Session is unavailable";

pub struct DaemonState {
    tx: mpsc::UnboundedSender<SessionMsg>,
    /// Shared with the session's pane readers: attached terminals' backlogs pace their output.
    pacing: Arc<Pacing>,
    shutdown: watch::Sender<bool>,
}

impl DaemonState {
    /// Starts a session whose durable state lives in `state_dir`. Pipes are loaded and, when
    /// configured, the last unclean snapshot is restored before this returns.
    pub async fn new(
        session: String,
        endpoint: String,
        config: Config,
        state_dir: PathBuf,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready, ready_rx) = oneshot::channel();
        let (shutdown, _) = watch::channel(false);
        let pacing = Arc::new(Pacing::default());
        let boot = session::Boot {
            session,
            endpoint,
            config,
            state_dir,
            tx: tx.clone(),
            ready,
            pacing: Arc::clone(&pacing),
        };
        tokio::spawn(session::run(boot, rx));
        let _ = ready_rx.await;
        Arc::new(Self {
            tx,
            pacing,
            shutdown,
        })
    }

    async fn ask<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> SessionMsg,
    ) -> Result<T, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(make(reply))
            .map_err(|_| SESSION_GONE.to_string())?;
        rx.await.map_err(|_| SESSION_GONE.to_string())
    }

    async fn session_req(&self, req: Request) -> Response {
        match self
            .ask(|reply| SessionMsg::Req {
                req: Box::new(req),
                reply,
            })
            .await
        {
            Ok(resp) => resp,
            Err(e) => Response::Error(e),
        }
    }

    async fn lookup(&self, target: &str) -> Result<PaneHandle, String> {
        let target = target.to_string();
        self.ask(|reply| SessionMsg::Lookup { target, reply })
            .await?
    }

    async fn subscribe_node(
        &self,
        name: String,
        since: Option<u64>,
        sink: Sink,
    ) -> Result<(), String> {
        self.ask(|reply| SessionMsg::SubscribeNode {
            name,
            since,
            sink,
            reply,
        })
        .await?
    }

    async fn subscribe(&self, filter: Filter, sink: Sink) -> Result<(), String> {
        self.ask(|reply| SessionMsg::Subscribe {
            filter,
            sink,
            reply,
        })
        .await
    }

    async fn idem_begin(&self, key: &str) -> Result<Claim<Response>, String> {
        let key = key.to_string();
        self.ask(|reply| SessionMsg::IdemBegin { key, reply }).await
    }

    fn idem_finish(&self, key: &str, value: Response) {
        let _ = self.tx.send(SessionMsg::IdemFinish {
            key: key.to_string(),
            value: Box::new(value),
        });
    }

    fn idem_abandon(&self, key: &str) {
        let _ = self.tx.send(SessionMsg::IdemAbandon {
            key: key.to_string(),
        });
    }

    fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
}

async fn send_message<S: AsyncStream>(
    framed: &mut Framed<S, LinesCodec>,
    msg: &DaemonMessage,
) -> bool {
    match serde_json::to_string(msg) {
        Ok(json) => framed.send(json).await.is_ok(),
        Err(_) => false,
    }
}

/// Acknowledges a subscription and forwards its events until the subscriber or the stream ends.
async fn stream_events<S: AsyncStream>(
    framed: &mut Framed<S, LinesCodec>,
    registered: Result<(), String>,
    mut rx: mpsc::Receiver<StreamEvent>,
    backlog: Option<Arc<Backlog>>,
    binary: bool,
) {
    let _release = CloseOnDrop(backlog.clone());
    if let Err(e) = registered {
        send_message(framed, &DaemonMessage::Response(Response::Error(e))).await;
        return;
    }
    let accepted = if binary {
        Response::BinaryStream
    } else {
        Response::Ok
    };
    if !send_message(framed, &DaemonMessage::Response(accepted)).await {
        return;
    }
    if binary {
        stream_frames(framed.get_mut(), &mut rx, backlog.as_deref()).await;
        return;
    }
    while let Some(event) = rx.recv().await {
        if !send_message(framed, &DaemonMessage::Event(event)).await {
            break;
        }
        if let Some(backlog) = &backlog {
            backlog.sent(now_ms());
        }
    }
}

/// Bytes of frames gathered into one write while more events are already queued.
const FRAME_BATCH: usize = 256 * 1024;

/// Writes events as length-prefixed binary frames, every event already queued in one write.
async fn stream_frames<S: AsyncStream>(
    io: &mut S,
    rx: &mut mpsc::Receiver<StreamEvent>,
    backlog: Option<&Backlog>,
) {
    let mut buf = Vec::with_capacity(64 * 1024);
    while let Some(first) = rx.recv().await {
        buf.clear();
        let mut count = 0;
        let mut next = Some(first);
        while let Some(event) = next.take() {
            push_frame(&mut buf, &event);
            count += 1;
            if buf.len() < FRAME_BATCH {
                next = rx.try_recv().ok();
            }
        }
        if io.write_all(&buf).await.is_err() {
            return;
        }
        if let Some(backlog) = backlog {
            let now = now_ms();
            (0..count).for_each(|_| backlog.sent(now));
        }
    }
}

/// Appends `event` as one frame: a big-endian u32 length, then its payload.
fn push_frame(buf: &mut Vec<u8>, event: &StreamEvent) {
    let start = buf.len();
    buf.extend_from_slice(&[0; 4]);
    match wire::encode_event(event, buf) {
        Ok(()) => {
            let len = (buf.len() - start - 4) as u32;
            buf[start..start + 4].copy_from_slice(&len.to_be_bytes());
        }
        Err(e) => {
            warn!("dropped an event that cannot be framed: {}", e);
            buf.truncate(start);
        }
    }
}

/// Serves a `Subscribe` request for the rest of the connection.
async fn subscribe_node<S: AsyncStream>(
    state: &DaemonState,
    framed: &mut Framed<S, LinesCodec>,
    request: Request,
) {
    let Request::Subscribe {
        name,
        since,
        binary,
        active_window,
        layout,
    } = request
    else {
        return;
    };
    let session_wide = Filter::subscriber(active_window, layout);
    // A session-wide subscriber is an attached terminal: its backlog paces pane output.
    let backlog = (name.is_none() && since.is_none()).then(|| state.pacing.track(now_ms()));
    let (sink, rx) = Sink::channel(backlog.clone());
    let registered = match (name, since) {
        (None, Some(_)) => Err("since requires a node name".to_string()),
        (Some(name), since) => state.subscribe_node(name, since, sink).await,
        (None, None) => state.subscribe(session_wide, sink).await,
    };
    stream_events(framed, registered, rx, backlog, binary).await;
}

pub async fn handle_client<S: AsyncStream>(state: Arc<DaemonState>, stream: S) {
    let mut framed = Framed::new(stream, LinesCodec::new());

    while let Some(line_res) = framed.next().await {
        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                error!("Error reading line from client: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err = DaemonMessage::Response(Response::Error(format!(
                    "Invalid request JSON: {}",
                    e
                )));
                if !send_message(&mut framed, &err).await {
                    break;
                }
                continue;
            }
        };

        match req {
            subscribe @ Request::Subscribe { .. } => {
                subscribe_node(&state, &mut framed, subscribe).await;
                break;
            }
            req => {
                let shutdown = matches!(req, Request::Shutdown);
                let resp = dispatch::dispatch(&state, req).await;
                if !send_message(&mut framed, &DaemonMessage::Response(resp)).await {
                    break;
                }
                if shutdown {
                    state.request_shutdown();
                    break;
                }
            }
        }
    }
}

/// Serves `endpoint` for `session` with `config`, keeping durable state under `state_dir`
/// (normally `store::session_dir(session)`).
pub async fn run_daemon(
    endpoint: &str,
    session: &str,
    state_dir: &Path,
    config: Config,
) -> std::io::Result<()> {
    #[cfg(unix)]
    let listener = IpcListener::bind(endpoint)?;
    #[cfg(windows)]
    let mut listener = IpcListener::bind(endpoint)?;

    info!("mpx daemon listening on {}", endpoint);
    let state = DaemonState::new(
        session.to_string(),
        endpoint.to_string(),
        config,
        state_dir.to_path_buf(),
    )
    .await;
    let mut shutdown = state.shutdown_signal();

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    let state_clone = Arc::clone(&state);
                    tokio::spawn(async move {
                        handle_client(state_clone, stream).await;
                    });
                }
                Err(e) => error!("Accept error: {}", e),
            },
            _ = shutdown.wait_for(|stop| *stop) => break,
        }
    }
    info!("mpx daemon on {} stopped", endpoint);
    Ok(())
}
