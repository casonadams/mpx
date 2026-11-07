use futures_util::{SinkExt, StreamExt};
use tokio_util::codec::{Framed, LinesCodec};

use crate::ipc::{IpcStream, connect_stream};
use crate::layout::{Divider, Rect, WindowTab};
use crate::protocol::{DaemonMessage, Request, Response};

/// Sends a request and yields the payload of the response variant `$variant` picks out; any other
/// response is an error for `$op`.
macro_rules! request {
    ($self:ident, $op:expr, $req:expr, $variant:pat => $value:expr) => {
        match $self.call($req).await? {
            $variant => Ok($value),
            other => Err($crate::client::unexpected($op, other)),
        }
    };
}

mod attach;
mod buffer;
mod connection;
mod node;
mod pane;
mod session;
mod stream;
mod window;

pub use connection::Handshake;
pub use session::ShellOutput;

pub type LayoutTuple = (Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>);

pub struct Client {
    framed: Framed<IpcStream, LinesCodec>,
    /// Idempotency key applied to the next request only.
    pending_key: Option<String>,
}

fn unexpected(op: &str, resp: Response) -> String {
    match resp {
        // A daemon from an older mpx does not know the request.
        Response::Error(e) if e.contains("unknown variant") => format!(
            "{op} needs a newer daemon: restart this session (mpx kill-server, then mpx restore)"
        ),
        Response::Error(e) => e,
        other => format!("Invalid response to {}: {:?}", op, other),
    }
}

impl Client {
    pub async fn connect(endpoint: &str) -> Result<Self, String> {
        let stream = connect_stream(endpoint)
            .await
            .map_err(|e| format!("Failed to connect to daemon at '{}': {}", endpoint, e))?;
        Ok(Self {
            framed: Framed::new(stream, LinesCodec::new()),
            pending_key: None,
        })
    }

    /// Makes the next request idempotent under `key`: the daemon runs it at most once per key and
    /// returns the first response to repeats, so retries are safe.
    pub fn idempotency_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.pending_key = Some(key.into());
        self
    }

    pub async fn call(&mut self, req: Request) -> Result<Response, String> {
        let req = match self.pending_key.take() {
            Some(key) => Request::Idempotent {
                key,
                request: Box::new(req),
            },
            None => req,
        };
        let req_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.framed.send(req_str).await.map_err(|e| e.to_string())?;
        let line = self
            .framed
            .next()
            .await
            .ok_or_else(|| "Connection closed by daemon".to_string())?
            .map_err(|e| e.to_string())?;
        match serde_json::from_str(&line).map_err(|e| format!("Failed to parse response: {}", e))? {
            DaemonMessage::Response(resp) => Ok(resp),
            DaemonMessage::Event(_) => {
                Err("Unexpected stream event in request-response".to_string())
            }
        }
    }

    pub async fn call_ok(&mut self, op: &str, req: Request) -> Result<(), String> {
        request!(self, op, req, Response::Ok => ())
    }

    /// Sends `req` and takes the payload of its response with `take`; another response is an error.
    async fn call_as<T>(
        &mut self,
        op: &str,
        req: Request,
        take: fn(Response) -> Option<T>,
    ) -> Result<T, String> {
        let response = self.call(req).await?;
        take(response).ok_or_else(|| format!("unexpected response to {op}"))
    }
}
