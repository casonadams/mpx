//! Pure status-line helpers: exit badges, agent badge text and the OSC 9 notification sequence.
use crate::layout::{Rect, WindowTab};
use crate::protocol::NodeInfo;

/// The node a pane name stands for: the newest with that name, since an exited process can leave
/// an older node of the same name in the session's history.
pub fn node_named<'a>(nodes: &'a [NodeInfo], name: &str) -> Option<&'a NodeInfo> {
    nodes
        .iter()
        .filter(|n| n.name == name)
        .max_by_key(|n| n.id)
}

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

/// Seconds east of UTC of the local time zone at `unix_ms` (0 when it cannot be determined).
pub fn local_utc_offset(unix_ms: u64) -> i64 {
    #[cfg(unix)]
    {
        let t = (unix_ms / 1000) as libc::time_t;
        // SAFETY: localtime_r only writes into the zeroed `tm` we own.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
    #[cfg(not(unix))]
    {
        let _ = unix_ms;
        0
    }
}

/// `(hours, minutes, seconds)` of the day for `unix_ms` shifted by `utc_offset_secs`.
pub fn time_of_day(unix_ms: u64, utc_offset_secs: i64) -> (u64, u64, u64) {
    let secs = ((unix_ms / 1000) as i64 + utc_offset_secs).rem_euclid(86_400) as u64;
    (secs / 3600, (secs / 60) % 60, secs % 60)
}

/// `HH:MM:SS` (24-hour) of `unix_ms` at `utc_offset_secs`.
pub fn clock_hms(unix_ms: u64, utc_offset_secs: i64) -> String {
    let (h, m, s) = time_of_day(unix_ms, utc_offset_secs);
    format!("{h:02}:{m:02}:{s:02}")
}

/// `hh:MM:SS` (12-hour, like tmux `%I:%M:%S`) of `unix_ms` at `utc_offset_secs`.
pub fn clock_12h(unix_ms: u64, utc_offset_secs: i64) -> String {
    let (h, m, s) = time_of_day(unix_ms, utc_offset_secs);
    let h12 = match h % 12 {
        0 => 12,
        h => h,
    };
    format!("{h12:02}:{m:02}:{s:02}")
}

/// Status-row notice shown when a session was restored from a snapshot at `restored_ms`.
pub fn restored_notice(restored_ms: u64, utc_offset_secs: i64) -> String {
    format!(
        "restored session from {}",
        clock_hms(restored_ms, utc_offset_secs)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusTarget<'a> {
    Window(usize),
    Pane(&'a str),
}

/// Identifies which window tab or pane name on the status line was clicked at `col`.
pub fn status_hit<'a>(
    session: &str,
    tabs: &'a [WindowTab],
    panes: &'a [(String, Rect, bool)],
    zoomed: bool,
    col: usize,
) -> Option<StatusTarget<'a>> {
    let mut offset = format!("[{session}] ").chars().count();
    if tabs.is_empty() {
        for (i, (name, _, is_focused)) in panes.iter().enumerate() {
            let flag = match (*is_focused, zoomed) {
                (true, true) => "*Z",
                (true, false) => "*",
                (false, _) => "-",
            };
            let text = format!("{}:{}{} ", i, name, flag);
            let len = text.chars().count();
            if col >= offset && col < offset + len {
                return Some(StatusTarget::Pane(name.as_str()));
            }
            offset += len;
        }
    } else {
        for tab in tabs {
            let flag = match (tab.is_active, zoomed) {
                (true, true) => "*Z",
                (true, false) => "*",
                (false, _) => "-",
            };
            let text = format!("{}:{}{} ", tab.index, tab.name, flag);
            let len = text.chars().count();
            if col >= offset && col < offset + len {
                return Some(StatusTarget::Window(tab.index));
            }
            offset += len;
        }
    }
    None
}
