mod dispatch;
mod pipes;
pub mod plan;
mod state;
mod wait;

use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info};

use crate::agents::{self, Harness};
use crate::config::Config;
use crate::ipc::{AsyncStream, IpcListener};
use crate::node;
use crate::protocol::{DaemonMessage, Request, Response, StreamEvent};

pub struct DaemonState {
    session: String,
    endpoint: String,
    config: Config,
    created_ms: u64,
    harnesses: Vec<Harness>,
    event_tx: broadcast::Sender<StreamEvent>,
    shutdown: watch::Sender<bool>,
    inner: Mutex<state::Inner>,
}

impl DaemonState {
    pub fn new(session: String, endpoint: String, config: Config) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(1024);
        let (shutdown, _) = watch::channel(false);
        let harnesses = agents::registry(&config);
        let state = Arc::new(Self {
            inner: Mutex::new(state::Inner::new(session.clone())),
            session,
            endpoint,
            config,
            created_ms: node::now_ms(),
            harnesses,
            event_tx,
            shutdown,
        });

        let listener = Arc::clone(&state);
        let mut rx = state.event_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(StreamEvent::Output { name, .. }) => listener.drain_alerts(&name),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        state
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.event_tx.subscribe()
    }

    fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
}

enum EventFilter {
    Node(Option<String>),
    Pipe(String),
}

impl EventFilter {
    fn matches(&self, event: &StreamEvent) -> bool {
        match (self, event) {
            (EventFilter::Node(None), _) => true,
            (EventFilter::Node(Some(want)), StreamEvent::Output { name, .. })
            | (EventFilter::Node(Some(want)), StreamEvent::Exit { name, .. })
            | (EventFilter::Node(Some(want)), StreamEvent::Spawned { name, .. }) => want == name,
            (EventFilter::Node(Some(_)), StreamEvent::Pipe { .. }) => false,
            (EventFilter::Pipe(want), StreamEvent::Pipe { name, .. }) => want == name,
            (EventFilter::Pipe(_), _) => false,
        }
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

async fn stream_events<S: AsyncStream>(
    framed: &mut Framed<S, LinesCodec>,
    state: &DaemonState,
    filter: EventFilter,
) {
    let mut rx = state.subscribe();
    if !send_message(framed, &DaemonMessage::Response(Response::Ok)).await {
        return;
    }
    loop {
        match rx.recv().await {
            Ok(event) => {
                if filter.matches(&event)
                    && !send_message(framed, &DaemonMessage::Event(event)).await
                {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
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
            Request::Subscribe { name } => {
                stream_events(&mut framed, &state, EventFilter::Node(name)).await;
                break;
            }
            Request::SubscribePipe { name } => {
                stream_events(&mut framed, &state, EventFilter::Pipe(name)).await;
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

pub async fn run_daemon(endpoint: &str, session: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    let listener = IpcListener::bind(endpoint)?;
    #[cfg(windows)]
    let mut listener = IpcListener::bind(endpoint)?;

    info!("mpx daemon listening on {}", endpoint);
    let state = DaemonState::new(
        session.to_string(),
        endpoint.to_string(),
        Config::load_or_default(),
    );
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
