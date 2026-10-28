//! Request routing for one connection. Requests that wait (timed waits, pipe reads, kill and
//! shutdown) run here so the session actor never blocks on them.
use std::sync::Arc;
use std::time::Duration;

use super::idempotency::Claim;
use super::pipes::ReadArgs;
use super::session::{SessionMsg, reply};
use super::{DaemonState, KILL_GRACE};
use crate::protocol::{Request, Response};

pub(super) async fn dispatch(state: &Arc<DaemonState>, req: Request) -> Response {
    match req {
        Request::Idempotent { key, request } => idempotent(state, key, *request).await,
        req => execute(state, req).await,
    }
}

async fn idempotent(state: &Arc<DaemonState>, key: String, req: Request) -> Response {
    if matches!(
        req,
        Request::Idempotent { .. } | Request::Subscribe { .. } | Request::SubscribePipe { .. }
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
        Request::ReadPipe {
            name,
            since,
            consumer,
            wait_ms,
            limit,
        } => {
            let args = ReadArgs {
                since,
                consumer,
                wait_ms,
                limit,
            };
            reply(state.pipes().read(&name, args).await, |(messages, next)| {
                Response::PipeMessages { messages, next }
            })
        }
        Request::Subscribe { .. } | Request::SubscribePipe { .. } => {
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

async fn wait(
    state: &Arc<DaemonState>,
    name: &str,
    condition: crate::protocol::WaitCondition,
    timeout_ms: Option<u64>,
) -> Response {
    let handle = match state.lookup(name).await {
        Ok(handle) => handle,
        Err(e) => return Response::Error(e),
    };
    reply(
        handle
            .wait(condition, timeout_ms.map(Duration::from_millis))
            .await,
        Response::WaitResult,
    )
}
