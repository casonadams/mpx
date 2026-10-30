//! Pure status-line helpers: exit badges and clock formatting.
use super::render::tab_text;
use super::statusline::{Item, Seg, Target, compose};
use crate::config::Justify;
use crate::layout::{Rect, WindowTab};
use crate::protocol::NodeInfo;
/// The node a pane name stands for: the newest with that name, since an exited process can leave
/// an older node of the same name in the session's history.
pub fn node_named<'a>(nodes: &'a [NodeInfo], name: &str) -> Option<&'a NodeInfo> {
    nodes.iter().filter(|n| n.name == name).max_by_key(|n| n.id)
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

/// Clickable window tabs of the built-in look, numbered from `window_base`.
pub fn tab_items(tabs: &[WindowTab], zoomed: bool, window_base: usize) -> Vec<Item> {
    tabs.iter()
        .map(|tab| Item {
            segs: vec![Seg::plain(tab_text(tab, zoomed, window_base))],
            target: Some(Target::Window(tab.index)),
        })
        .collect()
}

/// Clickable pane names of the built-in look, shown when the session has no windows.
pub fn pane_items(panes: &[(String, Rect, bool)], zoomed: bool, marked: Option<&str>) -> Vec<Item> {
    panes
        .iter()
        .enumerate()
        .map(|(i, (name, _, is_focused))| {
            let mut flag = match (*is_focused, zoomed) {
                (true, true) => "*Z".to_string(),
                (true, false) => "*".to_string(),
                (false, _) => "-".to_string(),
            };
            if marked == Some(name.as_str()) {
                flag.push('M');
            }
            Item {
                segs: vec![Seg::plain(format!("{i}:{name}{flag} "))],
                target: Some(Target::Pane(name.clone())),
            }
        })
        .collect()
}

/// Identifies which window tab or pane name on the status line was clicked at `col`, in the
/// built-in layout: `[session]` label, then the tabs (or pane names).
pub fn status_hit<'a>(
    session: &str,
    tabs: &[WindowTab],
    panes: &'a [(String, Rect, bool)],
    zoomed: bool,
    col: usize,
) -> Option<StatusTarget<'a>> {
    let items = if tabs.is_empty() {
        pane_items(panes, zoomed, None)
    } else {
        tab_items(tabs, zoomed, 0)
    };
    let line = compose(
        0,
        vec![Seg::plain(format!("[{session}] "))],
        items,
        "",
        Vec::new(),
        Justify::Left,
    );
    line.targets
        .into_iter()
        .find(|(start, end, _)| (*start..*end).contains(&col))
        .and_then(|(_, _, target)| match target {
            Target::Window(index) => Some(StatusTarget::Window(index)),
            Target::Pane(name) => panes
                .iter()
                .find(|(n, ..)| *n == name)
                .map(|(n, ..)| StatusTarget::Pane(n.as_str())),
        })
}
