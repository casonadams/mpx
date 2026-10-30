//! Request routing for one connection. Requests that wait (timed waits, pipe reads, log reads,
//! kill and shutdown) run here so the session actor never blocks on them.
use std::sync::Arc;
use std::time::Duration;

use super::idempotency::Claim;
use super::session::{SessionMsg, reply};
use super::{DaemonState, KILL_GRACE};
use crate::node::pane::LogQuery;
use crate::protocol::{ChannelAction, Request, Response};

pub(super) async fn dispatch(state: &Arc<DaemonState>, req: Request) -> Response {
    match req {
        Request::Idempotent { key, request } => idempotent(state, key, *request).await,
        req => execute(state, req).await,
    }
}

async fn idempotent(state: &Arc<DaemonState>, key: String, req: Request) -> Response {
    if matches!(
        req,
        Request::Idempotent { .. } | Request::Subscribe { .. }
    ) {
        return Response::Error("Idempotent requests cannot nest or stream".to_string());
    }
    match state.idem_begin(&key).await {
        Ok(Claim::Cached(resp)) => resp,
        Ok(Claim::Wait(rx)) => match rx.await {
            Ok(Some(resp)) => resp,
            _ => Response::Error(format!("idempotent request '{key}' did not complete")),
        },
        Ok(Claim::Run) => {
            let ticket = IdemTicket {
                state: Arc::clone(state),
                key,
                done: false,
            };
            let resp = execute(state, req).await;
            ticket.finish(resp.clone());
            resp
        }
        Err(e) => Response::Error(e),
    }
}

/// Owns the claim on an idempotency key. Dropped unfinished, it releases the key with no response.
struct IdemTicket {
    state: Arc<DaemonState>,
    key: String,
    done: bool,
}

impl IdemTicket {
    fn finish(mut self, value: Response) {
        self.done = true;
        self.state.idem_finish(&self.key, value);
    }
}

impl Drop for IdemTicket {
    fn drop(&mut self) {
        if !self.done {
            self.state.idem_abandon(&self.key);
        }
    }
}

/// Runs one request that is not an idempotent wrapper.
pub(super) async fn execute(state: &Arc<DaemonState>, req: Request) -> Response {
    match req {
        Request::Shutdown => shutdown(state).await,
        Request::Kill { name } => kill(state, &name).await,
        Request::KillWindow { index } => kill_window(state, index).await,
        Request::Wait {
            name,
            condition,
            timeout_ms,
        } => wait(state, &name, condition, timeout_ms).await,
        Request::WaitFor { channel, action } => wait_for(state, &channel, action).await,
        Request::Logs {
            name,
            tail,
            plain,
            since,
        } => logs(state, &name, LogQuery { tail, plain, since }).await,
        Request::Subscribe { .. } => {
            Response::Error("subscriptions require a streaming connection".to_string())
        }
        Request::Idempotent { .. } => {
            Response::Error("Idempotent requests cannot nest or stream".to_string())
        }
        req => state.session_req(req).await,
    }
}

/// Waits for the given exits, bounded by the kill grace period.
async fn await_exits(exits: Vec<tokio::sync::oneshot::Receiver<()>>) {
    let _ = tokio::time::timeout(KILL_GRACE, async {
        for exit in exits {
            let _ = exit.await;
        }
    })
    .await;
}

async fn shutdown(state: &Arc<DaemonState>) -> Response {
    match state.ask(|reply| SessionMsg::Shutdown { reply }).await {
        Ok(exits) => {
            await_exits(exits).await;
            Response::Ok
        }
        Err(e) => Response::Error(e),
    }
}

async fn kill(state: &Arc<DaemonState>, name: &str) -> Response {
    let target = name.to_string();
    match state.ask(|reply| SessionMsg::Kill { target, reply }).await {
        Ok(Ok(Some(exit))) => {
            await_exits(vec![exit]).await;
            Response::Ok
        }
        Ok(Ok(None)) => Response::Ok,
        Ok(Err(e)) | Err(e) => Response::Error(e),
    }
}

async fn kill_window(state: &Arc<DaemonState>, index: Option<usize>) -> Response {
    match state
        .ask(|reply| SessionMsg::KillWindow { index, reply })
        .await
    {
        Ok(Ok(exits)) => {
            await_exits(exits).await;
            Response::Ok
        }
        Ok(Err(e)) | Err(e) => Response::Error(e),
    }
}

/// Asked of the pane directly: a full plain read takes milliseconds, and the session must not
/// wait on it, as every pane's output queues behind the session while it waits, out of pacing's
/// sight, and would then reach an attached terminal in one burst that overflows it.
async fn logs(state: &Arc<DaemonState>, name: &str, query: LogQuery) -> Response {
    let read = async { state.lookup(name).await?.logs(query).await };
    reply(read.await, |(data, next)| Response::LogChunk { data, next })
}

async fn wait(
    state: &Arc<DaemonState>,
    name: &str,
    condition: crate::protocol::WaitCondition,
    timeout_ms: Option<u64>,
) -> Response {
    let timeout = timeout_ms.map(Duration::from_millis);
    let waited = async { state.lookup(name).await?.wait(condition, timeout).await };
    reply(waited.await, Response::WaitResult)
}

/// tmux `wait-for`: blocks this connection, never the session.
async fn wait_for(state: &Arc<DaemonState>, channel: &str, action: ChannelAction) -> Response {
    let blocked = {
        let mut channels = state.channels.lock().unwrap_or_else(|e| e.into_inner());
        match action {
            ChannelAction::Wait => channels.wait(channel),
            ChannelAction::Lock => channels.lock(channel),
            ChannelAction::Signal => {
                channels.signal(channel);
                None
            }
            ChannelAction::Unlock => {
                if let Err(e) = channels.unlock(channel) {
                    return Response::Error(e);
                }
                None
            }
        }
    };
    if let Some(released) = blocked {
        let _ = released.await;
    }
    Response::Ok
}
