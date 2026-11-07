mod buffer;
mod message;
mod node;
mod pane;
mod session;
mod window;

pub use buffer::*;
pub use message::*;
pub use node::*;
pub use pane::*;
pub use session::*;
pub use window::*;

/// Stable numeric pane identifier, rendered as `%N` (tmux-compatible).
pub type PaneId = u32;

/// Version of the client-daemon protocol. Bump it on any change an older peer cannot parse, so
/// clients refuse an incompatible daemon instead of misreading it.
pub const PROTOCOL: u32 = 16;

fn default_true() -> bool {
    true
}
