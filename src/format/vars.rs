use crate::layout::{Rect, WindowTab};
use crate::protocol::{Buffer, ClientInfo, NodeInfo};

use super::{PaneView, SessionTimes, Vars, flag};

/// tmux version reported by `#{version}`.
const TMUX_VERSION: &str = "3.4";

/// The `list-buffers -F` default: `name: N bytes: "sample"`.
pub const DEFAULT_BUFFER_FORMAT: &str =
    "#{buffer_name}: #{buffer_size} bytes: \"#{buffer_sample}\"";

/// Machine host name, empty when it cannot be read.
fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if !ok {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
}

/// Variables that do not depend on the pane or session being described.
fn shared_vars(vars: &mut Vars, session: &str, times: SessionTimes, attached: usize) {
    let host = host_name();
    let host_short = host.split('.').next().unwrap_or_default().to_string();
    let created = times.restored_ms.unwrap_or(times.started_ms);
    vars.extend([
        ("client_session", session.to_string()),
        ("session_id", format!("${}", times.started_ms)),
        ("session_created", (created / 1000).to_string()),
        ("start_time", (times.started_ms / 1000).to_string()),
        ("session_attached", attached.to_string()),
        ("host", host),
        ("host_short", host_short),
        ("pid", std::process::id().to_string()),
        ("version", TMUX_VERSION.to_string()),
    ]);
}

/// The variables of a session with no pane to describe: its name, times and the host's.
pub fn session_vars(session: &str, times: SessionTimes, attached: usize) -> Vars {
    let mut vars = Vars::from([("session_name", session.to_string())]);
    shared_vars(&mut vars, session, times, attached);
    vars
}

pub fn pane_vars(p: &PaneView) -> Vars {
    let mut vars = Vars::from([
        ("pane_id", format!("%{}", p.id)),
        ("pane_index", (p.index + p.pane_base).to_string()),
        ("pane_pid", p.pid.map(|x| x.to_string()).unwrap_or_default()),
        ("pane_title", p.title.clone()),
        ("pane_current_command", p.command.clone()),
        ("pane_current_path", p.path.clone()),
        ("pane_width", p.width.to_string()),
        ("pane_height", p.height.to_string()),
        ("pane_active", flag(p.active)),
        ("pane_dead", flag(p.dead)),
        ("pane_input_off", flag(p.input_off)),
        ("pane_marked", flag(p.marked)),
        ("pane_marked_set", flag(p.marked_set)),
        ("pane_in_mode", flag(p.in_mode)),
        (
            "pane_mode",
            if p.in_mode { "copy-mode" } else { "" }.to_string(),
        ),
        ("pane_tty", p.tty.clone()),
        ("pane_start_command", p.start_command.clone()),
        ("pane_at_left", flag(p.edges[0])),
        ("pane_at_top", flag(p.edges[1])),
        ("pane_at_right", flag(p.edges[2])),
        ("pane_at_bottom", flag(p.edges[3])),
        ("session_name", p.session.clone()),
        ("session_windows", p.session_windows.to_string()),
        ("window_index", (p.window + p.window_base).to_string()),
        ("window_id", format!("@{}", p.window)),
        ("window_name", p.window_name.clone()),
        ("window_active", flag(p.window_active)),
        ("window_panes", p.window_panes.to_string()),
        ("window_flags", WindowFlags::from(p).render()),
        ("window_zoomed_flag", flag(p.window_zoomed)),
        ("window_activity_flag", flag(p.window_activity)),
        ("window_bell_flag", flag(p.window_bell)),
        ("window_silence_flag", flag(p.window_silence)),
        ("window_last_flag", flag(p.window_last)),
        ("window_linked", flag(p.window_links.len() > 1)),
        ("window_linked_sessions_list", linked_sessions(p)),
        ("window_marked_flag", flag(p.window_marked)),
        ("pane_synchronized", flag(p.synchronized)),
    ]);
    if let Some(r) = p.rect {
        vars.extend([
            ("pane_left", r.x.to_string()),
            ("pane_top", r.y.to_string()),
            (
                "pane_right",
                r.x.saturating_add(r.width).saturating_sub(1).to_string(),
            ),
            (
                "pane_bottom",
                r.y.saturating_add(r.height).saturating_sub(1).to_string(),
            ),
        ]);
    }
    if let Some((x, y)) = p.cursor {
        vars.extend([("cursor_x", x.to_string()), ("cursor_y", y.to_string())]);
    }
    if let Some(lines) = p.history_size {
        vars.insert("history_size", lines.to_string());
    }
    shared_vars(&mut vars, &p.session, p.times, p.session_attached);
    vars
}

/// Variables of one attached client, for `list-clients` formats; `attached` is how many clients
/// its session has.
pub fn client_vars(info: &ClientInfo, attached: usize) -> Vars {
    let tty = if info.name.starts_with("/dev/") {
        info.name.clone()
    } else {
        String::new()
    };
    Vars::from([
        ("client_name", info.name.clone()),
        ("client_tty", tty),
        ("client_width", info.width.to_string()),
        ("client_height", info.height.to_string()),
        ("client_session", info.session.clone()),
        ("client_readonly", flag(info.readonly)),
        ("client_created", (info.created_ms / 1000).to_string()),
        ("client_activity", (info.activity_ms / 1000).to_string()),
        ("session_name", info.session.clone()),
        ("session_attached", attached.to_string()),
    ])
}

/// The tmux window flags of one window; `render` writes them in tmux's order.
#[derive(Default)]
pub(crate) struct WindowFlags {
    pub(crate) activity: bool,
    pub(crate) bell: bool,
    pub(crate) silence: bool,
    pub(crate) current: bool,
    pub(crate) zoomed: bool,
    pub(crate) last: bool,
    pub(crate) marked: bool,
}

impl WindowFlags {
    /// `#` activity, `!` bell, `~` silence, `*` current, `Z` zoomed, `-` last, `M` marked pane.
    pub(crate) fn render(&self) -> String {
        [
            (self.activity, '#'),
            (self.bell, '!'),
            (self.silence, '~'),
            (self.current, '*'),
            (self.zoomed, 'Z'),
            (self.last, '-'),
            (self.marked, 'M'),
        ]
        .into_iter()
        .filter_map(|(on, c)| on.then_some(c))
        .collect()
    }

    /// Flags of a tab; `zoomed` applies only to the active window.
    pub(crate) fn of_tab(tab: &WindowTab, zoomed: bool) -> Self {
        let (activity, bell, current) = (tab.has_activity, tab.has_bell, tab.is_active);
        Self {
            activity,
            bell,
            current,
            silence: tab.has_silence,
            zoomed: current && zoomed,
            last: tab.is_last,
            marked: tab.has_marked,
        }
    }
}

impl From<&PaneView> for WindowFlags {
    fn from(p: &PaneView) -> Self {
        Self {
            activity: p.window_activity,
            bell: p.window_bell,
            current: p.window_active,
            silence: p.window_silence,
            zoomed: p.window_zoomed,
            last: p.window_last,
            marked: p.window_marked,
        }
    }
}

/// `window_linked_sessions_list`: the names of the sessions a window is linked into, each once. This
/// daemon has one session, so it is that session's name while the window has an index, else empty.
fn linked_sessions(p: &PaneView) -> String {
    if p.window_links.is_empty() {
        String::new()
    } else {
        p.session.clone()
    }
}

/// The layout rectangle of the pane named `name`, and whether the layout marks it active.
fn layout_of(name: &str, rects: &[(String, Rect, bool)]) -> (Option<Rect>, bool) {
    rects
        .iter()
        .find(|(n, _, _)| n == name)
        .map_or((None, false), |(_, r, f)| (Some(*r), *f))
}

/// Copies the window-tab fields of `tab` into `view`; the fields stay at their defaults when it has none.
fn apply_tab(view: &mut PaneView, tab: Option<&WindowTab>) {
    if let Some(t) = tab {
        view.window_active = t.is_active;
        view.window_last = t.is_last;
        view.window_activity = t.has_activity;
        view.window_bell = t.has_bell;
        view.window_marked = t.has_marked;
        view.window_zoomed = t.zoomed;
        view.window_silence = t.has_silence;
        view.synchronized = t.synchronized;
        view.window_links = t.linked.clone();
    }
}

/// View of one pane from its node position, the current layout rectangles and the window tabs.
pub fn pane_view(
    session: &str,
    index: usize,
    node: &NodeInfo,
    rects: &[(String, Rect, bool)],
    tabs: &[WindowTab],
) -> PaneView {
    let window = node.window.unwrap_or(0);
    let (rect, layout_active) = layout_of(&node.name, rects);
    let (width, height) = rect.map_or((0, 0), |r| (r.width, r.height));
    let marked = node.marked.unwrap_or(false);
    let tab = tabs.iter().find(|t| t.index == window);
    let mut view = PaneView {
        session: session.to_string(),
        id: node.id,
        index,
        pid: node.pid,
        title: node.title.clone().unwrap_or_default(),
        command: node
            .foreground
            .clone()
            .unwrap_or_else(|| node.command.clone()),
        path: node.cwd.clone().unwrap_or_default(),
        width,
        height,
        active: tab.map_or(layout_active, |t| {
            t.focused.as_deref() == Some(node.name.as_str())
        }),
        dead: !node.is_running(),
        marked,
        marked_set: tabs.iter().any(|t| t.has_marked) || marked,
        input_off: node.input_off.unwrap_or(false),
        window,
        window_name: tab.map(|t| t.name.clone()).unwrap_or_default(),
        start_command: node.command.clone(),
        tty: node
            .tty
            .as_ref()
            .map(|t| format!("/dev/{t}"))
            .unwrap_or_default(),
        rect,
        edges: at_edges(rect, rects),
        cursor: node.cursor,
        history_size: node.history_size,
        session_windows: tabs.len(),
        ..Default::default()
    };
    apply_tab(&mut view, tab);
    view
}

/// Variables of one paste buffer, for `list-buffers -F`.
pub fn buffer_vars(buffer: &Buffer) -> Vars {
    Vars::from([
        ("buffer_name", buffer.name.clone()),
        ("buffer_size", buffer.data.len().to_string()),
        ("buffer_sample", buffer_sample(&buffer.data)),
    ])
}

/// The first 50 characters of a buffer, control characters shown as escapes.
pub fn buffer_sample(data: &str) -> String {
    const SAMPLE_CHARS: usize = 50;
    let mut out = String::new();
    for (i, c) in data.chars().enumerate() {
        if i == SAMPLE_CHARS {
            out.push_str("...");
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// Whether `rect` touches the left, top, right and bottom edges of the laid-out panes.
fn at_edges(rect: Option<Rect>, rects: &[(String, Rect, bool)]) -> [bool; 4] {
    let Some(r) = rect else {
        return [false; 4];
    };
    let left = rects.iter().map(|(_, q, _)| q.x).min().unwrap_or(r.x);
    let top = rects.iter().map(|(_, q, _)| q.y).min().unwrap_or(r.y);
    let right = rects
        .iter()
        .map(|(_, q, _)| q.x.saturating_add(q.width))
        .max()
        .unwrap_or(0);
    let bottom = rects
        .iter()
        .map(|(_, q, _)| q.y.saturating_add(q.height))
        .max()
        .unwrap_or(0);
    [
        r.x == left,
        r.y == top,
        r.x.saturating_add(r.width) == right,
        r.y.saturating_add(r.height) == bottom,
    ]
}
