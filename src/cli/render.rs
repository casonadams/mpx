//! Text and JSON rendering of CLI listings. Pure: no I/O.
use serde::Serialize;

use crate::protocol::{NodeInfo, PipeInfo, PipeMessage, StreamEvent};

const PANE_RULE: usize = 86;
const PIPE_RULE: usize = 30;

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

/// `mpx pipe read` output: raw message data, or one compact JSON object.
pub fn pipe_read(messages: &[PipeMessage], next: u64, json: bool) -> Result<String, String> {
    if json {
        let value = serde_json::json!({ "messages": messages, "next": next });
        return serde_json::to_string(&value)
            .map(|s| format!("{s}\n"))
            .map_err(|e| e.to_string());
    }
    Ok(messages.iter().map(|m| m.data.as_str()).collect())
}

/// `mpx pipe list` output.
pub fn pipes(pipes: &[PipeInfo], json: bool, session: &str) -> Result<String, String> {
    if json {
        return json_pretty(pipes);
    }
    if pipes.is_empty() {
        return Ok(format!("No virtual pipes in session '{session}'\n"));
    }
    let mut out = format!("PIPES ({})\n{}\n", pipes.len(), "-".repeat(PIPE_RULE));
    out.extend(pipes.iter().map(|p| format!("{}\n", p.name)));
    Ok(out)
}

/// Line printed by `mpx pipe subscribe` for one stream event; other events print nothing.
pub fn pipe_event(event: &StreamEvent) -> Option<String> {
    match event {
        StreamEvent::Pipe { message, .. } => Some(format!("{}\n", message.data)),
        _ => None,
    }
}
