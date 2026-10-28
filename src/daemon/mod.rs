mod dispatch;
pub mod idempotency;
mod pipes;
pub mod plan;
mod session;
pub mod snapshot;

use futures_util::{SinkExt, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info};

use crate::config::Config;
use crate::ipc::{AsyncStream, IpcListener};
use crate::node::pane::PaneHandle;
use crate::node::sink::Sink;
use crate::protocol::{DaemonMessage, Request, Response, StreamEvent};
use crate::store;
use idempotency::Claim;
use session::{Filter, SessionMsg};

pub(super) const KILL_GRACE: Duration = Duration::from_secs(2);
const SESSION_GONE: &str = "Session is unavailable";

pub struct DaemonState {
    tx: mpsc::UnboundedSender<SessionMsg>,
    pipes: pipes::PipesHandle,
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
        let pipe_store = config
            .persist_pipes
            .then(|| store::PipeStore::new(state_dir.join("pipes")));
        let pipes = pipes::spawn(pipe_store);
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready, ready_rx) = oneshot::channel();
        let (shutdown, _) = watch::channel(false);
        let boot = session::Boot {
            session,
            endpoint,
            config,
            state_dir,
            tx: tx.clone(),
            pipes: pipes.clone(),
            ready,
        };
        tokio::spawn(session::run(boot, rx));
        let _ = ready_rx.await;
        Arc::new(Self {
            tx,
            pipes,
            shutdown,
        })
    }

    fn pipes(&self) -> &pipes::PipesHandle {
        &self.pipes
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
) {
    if let Err(e) = registered {
        send_message(framed, &DaemonMessage::Response(Response::Error(e))).await;
        return;
    }
    if !send_message(framed, &DaemonMessage::Response(Response::Ok)).await {
        return;
    }
    while let Some(event) = rx.recv().await {
        if !send_message(framed, &DaemonMessage::Event(event)).await {
            break;
        }
    }
}

async fn subscribe_node<S: AsyncStream>(
    state: &DaemonState,
    framed: &mut Framed<S, LinesCodec>,
    name: Option<String>,
    since: Option<u64>,
) {
    let (sink, rx) = Sink::channel();
    let registered = match (name, since) {
        (None, Some(_)) => Err("since requires a node name".to_string()),
        (Some(name), since) => state.subscribe_node(name, since, sink).await,
        (None, None) => state.subscribe(Filter::Everything, sink).await,
    };
    stream_events(framed, registered, rx).await;
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
            Request::Subscribe { name, since } => {
                subscribe_node(&state, &mut framed, name, since).await;
                break;
            }
            Request::SubscribePipe { name } => {
                let (sink, rx) = Sink::channel();
                let registered = state.subscribe(Filter::Pipe(name), sink).await;
                stream_events(&mut framed, registered, rx).await;
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

/// Serves `endpoint` for `session`, keeping durable state under the session's state directory.
pub async fn run_daemon(endpoint: &str, session: &str) -> std::io::Result<()> {
    run_daemon_in(endpoint, session, &store::session_dir(session)).await
}

/// Serves `endpoint` for `session`, keeping durable state under `state_dir`.
pub async fn run_daemon_in(endpoint: &str, session: &str, state_dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    let listener = IpcListener::bind(endpoint)?;
    #[cfg(windows)]
    let mut listener = IpcListener::bind(endpoint)?;

    info!("mpx daemon listening on {}", endpoint);
    let state = DaemonState::new(
        session.to_string(),
        endpoint.to_string(),
        Config::load_or_default(),
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
