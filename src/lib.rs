pub mod buffer;
pub mod cli;
pub mod client;
pub mod daemon;
pub mod ipc;
pub mod node;
pub mod protocol;
pub mod layout;
pub mod vt;
pub mod tui;
pub mod pipe;
pub mod mcp;

pub use protocol::{Request, Response, StreamEvent};
