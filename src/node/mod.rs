//! Panes. Each pane is an actor (`pane`) that owns its process handle, output buffer, screen,
//! agent state, subscribers and waiters. Blocking PTY reads and child waits run on threads that
//! only forward messages to the actor.
pub mod actor;
pub mod pacing;
pub mod pane;
pub mod process;
pub mod pty;
pub mod sink;
pub mod task;
pub mod waits;

use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::protocol::PaneId;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Process not running")]
    NotRunning,
    #[error("Failed to write to stdin: {0}")]
    StdinFailed(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Everything needed to start a node process.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub name: String,
    pub id: PaneId,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// Extra environment, applied after the terminal defaults.
    pub env: Vec<(String, String)>,
}

/// Resolves at `at`, or never without a deadline: a `select!` arm for an optional timer.
pub async fn sleep_until_opt(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The process of a pane, as seen by its actor. Output and exit arrive through the pane mailbox.
pub trait Process: Send {
    fn pid(&self) -> Option<u32>;

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError>;

    fn kill(&self) -> Result<(), NodeError>;
}
