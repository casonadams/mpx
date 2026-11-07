use futures_util::{SinkExt, StreamExt};

use super::Client;
use crate::protocol::{DaemonMessage, PROTOCOL, Request, Response};

const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// What answered on a session socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    /// A daemon speaking this client's protocol.
    Compatible,
    /// A daemon this client cannot talk to, typically from another mpx version; `pid` when known.
    Incompatible { pid: Option<u32> },
    /// Nothing answered.
    Gone,
}

impl Client {
    /// Checks that the daemon speaks this client's protocol. Must be the first request.
    pub async fn handshake(&mut self) -> Handshake {
        let peer = self.peer_pid();
        let Ok(req) = serde_json::to_string(&Request::Info) else {
            return Handshake::Gone;
        };
        if self.framed.send(req).await.is_err() {
            return Handshake::Gone;
        }
        let line = match tokio::time::timeout(HANDSHAKE_TIMEOUT, self.framed.next()).await {
            Ok(Some(Ok(line))) => line,
            Ok(Some(Err(_))) => return Handshake::Incompatible { pid: peer },
            _ => return Handshake::Gone,
        };
        match serde_json::from_str(&line) {
            Ok(DaemonMessage::Response(Response::Session(info))) if info.protocol == PROTOCOL => {
                Handshake::Compatible
            }
            Ok(DaemonMessage::Response(Response::Session(info))) => Handshake::Incompatible {
                pid: Some(info.pid),
            },
            _ => Handshake::Incompatible { pid: peer },
        }
    }

    #[cfg(unix)]
    fn peer_pid(&self) -> Option<u32> {
        let pid = self.framed.get_ref().peer_cred().ok()?.pid()?;
        u32::try_from(pid).ok()
    }

    #[cfg(not(unix))]
    fn peer_pid(&self) -> Option<u32> {
        None
    }

    pub async fn shutdown(&mut self) -> Result<(), String> {
        let _ = tokio::time::timeout(
            SHUTDOWN_TIMEOUT,
            self.call_ok("Shutdown", Request::Shutdown),
        )
        .await;
        Ok(())
    }
}
