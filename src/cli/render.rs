//! Text and JSON rendering of CLI listings. Pure: no I/O.
use serde::Serialize;

use crate::protocol::NodeInfo;

const PANE_RULE: usize = 86;

fn json_pretty<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|s| format!("{s}\n"))
        .map_err(|e| e.to_string())
}

fn pane_row(node: &NodeInfo) -> String {
    let node_type = if node.pty { "pty" } else { "task" };
    let pid = node
        .pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    format!(
        "{:<6} {:<16} {:<8} {:<8} {:<14} {:<30}\n",
        node.pane_ref(),
        node.name,
        pid,
        node_type,
        node.status_label(),
        node.command
    )
}

/// `mpx list-panes` output: a table, or pretty JSON of the nodes.
pub fn panes(nodes: &[NodeInfo], json: bool) -> Result<String, String> {
    if json {
        return json_pretty(nodes);
    }
    let mut out = format!(
        "{:<6} {:<16} {:<8} {:<8} {:<14} {:<30}\n",
        "ID", "NAME", "PID", "TYPE", "STATUS", "COMMAND"
    );
    out.push_str(&"-".repeat(PANE_RULE));
    out.push('\n');
    out.extend(nodes.iter().map(pane_row));
    Ok(out)
}

/// What `mpx logs -f` does with one output chunk, given the byte offset printed so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowStep {
    /// The chunk continues the output: print it.
    Print,
    /// Already printed.
    Skip,
    /// Bytes were missed (or the chunk overlaps): re-read the log from the cursor.
    Resync,
}

/// Decides how a chunk spanning `offset..next` applies after `cursor` bytes were printed.
pub fn follow_step(cursor: u64, offset: u64, next: u64) -> FollowStep {
    if next <= cursor {
        FollowStep::Skip
    } else if offset == cursor {
        FollowStep::Print
    } else {
        FollowStep::Resync
    }
}
