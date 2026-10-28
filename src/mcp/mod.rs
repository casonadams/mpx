//! Model Context Protocol server exposing mpx sessions, panes, pipes and agents as tools.
mod exec;
pub mod server;
pub mod tools;

pub use exec::{PollStep, poll_step};
pub use server::McpServer;
pub use tools::{Call, parse_call, tool_list, tool_names};
