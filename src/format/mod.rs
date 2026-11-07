use std::collections::BTreeMap;

use crate::layout::Rect;

mod expand;
mod time;
mod vars;

pub(crate) use expand::backslash_escape;
pub use expand::{expand, expand_with};
pub use time::utc_offset_secs;
pub(crate) use vars::WindowFlags;
pub use vars::{
    DEFAULT_BUFFER_FORMAT, buffer_sample, buffer_vars, client_vars, pane_vars, pane_view,
    session_vars,
};

/// Format variables available to `#{name}` references.
pub type Vars = BTreeMap<&'static str, String>;

/// The lists the loop formats `#{W:...}`, `#{P:...}` and `#{S:...}` iterate over: one entry of
/// variables per window, pane and session.
#[derive(Debug, Clone, Default)]
pub struct Lists {
    pub windows: Vec<Vars>,
    pub panes: Vec<Vars>,
    pub sessions: Vec<Vars>,
}

/// Everything a format needs to describe one pane.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneView {
    pub session: String,
    pub id: u32,
    pub index: usize,
    pub pid: Option<u32>,
    pub title: String,
    pub command: String,
    pub path: String,
    pub width: u16,
    pub height: u16,
    pub active: bool,
    pub dead: bool,
    pub marked: bool,
    /// Input to the pane is off (`select-pane -d`).
    pub input_off: bool,
    pub marked_set: bool,
    /// The pane is in a mode (copy mode), (`pane_in_mode`).
    pub in_mode: bool,
    pub window: usize,
    pub window_name: String,
    /// Command the pane was started with (`#{pane_start_command}`).
    pub start_command: String,
    /// Device path of the pane's terminal, empty when unknown.
    pub tty: String,
    /// Position in the current layout, `None` when the pane is not laid out.
    pub rect: Option<Rect>,
    pub window_active: bool,
    pub window_last: bool,
    /// The indices the window is linked at, in number order; one entry unless it is linked elsewhere.
    pub window_links: Vec<usize>,
    pub window_activity: bool,
    pub window_bell: bool,
    /// The window holds the marked pane.
    pub window_marked: bool,
    /// The window is zoomed (tmux `window_zoomed_flag`).
    pub window_zoomed: bool,
    /// The window has been quiet for `monitor-silence` seconds (tmux `window_silence_flag`).
    pub window_silence: bool,
    /// The window's panes all receive typed input (`synchronize-panes`).
    pub synchronized: bool,
    /// The pane touches the left, top, right and bottom edge of the window (`pane_at_*`).
    pub edges: [bool; 4],
    /// Cursor column and row of the pane's screen (`cursor_x`, `cursor_y`); `None` for a pane without a terminal.
    pub cursor: Option<(u16, u16)>,
    /// Scrollback lines the pane's captures reach (`history_size`); `None` for a pane without a terminal.
    pub history_size: Option<usize>,
    /// Number of panes in this pane's window.
    pub window_panes: usize,
    /// Number of windows in the session.
    pub session_windows: usize,
    /// `base-index`: the number of the first window, added to `window` for `#{window_index}`.
    pub window_base: usize,
    /// `pane-base-index`: the number of the first pane, added to `index` for `#{pane_index}`.
    pub pane_base: usize,
    /// Clients attached to the pane's session (`#{session_attached}`).
    pub session_attached: usize,
    /// When the session's daemon started and whether it restored a snapshot (`session_id`,
    /// `session_created`, `start_time`).
    pub times: SessionTimes,
}

/// When a session's daemon started and whether it restored a snapshot: what the session's time
/// variables show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionTimes {
    /// Unix ms the daemon started.
    pub started_ms: u64,
    /// Unix ms the snapshot was restored, when it was.
    pub restored_ms: Option<u64>,
}

/// Whether a format's value is true: non-empty and not `0` (the rule of `#{?...}` and `if-shell -F`).
pub fn truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

/// tmux's `1`/`0` rendering of a boolean variable.
fn flag(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}
