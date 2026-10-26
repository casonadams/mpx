pub mod pty;
pub mod task;

use std::pin::Pin;
use thiserror::Error;

use crate::protocol::NodeInfo;

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

pub trait Node: Send + Sync {
    fn name(&self) -> &str;
    fn info(&self) -> NodeInfo;
    fn send_input(&self, data: &[u8]) -> Result<(), NodeError>;
    fn read_buffer(&self, tail: Option<usize>) -> String;
    fn read_screen(&self) -> String;
    fn resize(&self, cols: u16, rows: u16) -> Result<(), NodeError>;
    fn cursor(&self) -> (usize, usize, bool);
    fn cwd(&self) -> Option<String>;
    fn kill(&self) -> Result<(), NodeError>;
    fn wait_exit<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<i32>> + Send + 'a>>;
    fn is_running(&self) -> bool;
}
