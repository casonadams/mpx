//! Pure status-line helpers: exit badges, agent badge text and the OSC 9 notification sequence.
use crate::protocol::NodeInfo;

/// `[exited N]` for a pane whose process ended and which is kept on screen.
pub fn exit_badge(node: &NodeInfo) -> Option<String> {
    if node.is_running() {
        return None;
    }
    Some(match node.exit_code {
        Some(code) => format!("[exited {code}]"),
        None => "[exited]".to_string(),
    })
}

/// `◆N` when N agents are waiting for input.
pub fn agent_badge(waiting: usize) -> Option<String> {
    (waiting > 0).then(|| format!("◆{waiting}"))
}

/// OSC 9 desktop notification announcing that a named agent needs input.
pub fn osc9_needs_input(name: &str) -> String {
    format!("\x1b]9;mpx: {name} needs input\x07")
}
