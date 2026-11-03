//! Status line composition: tmux formats and `#[style]` directives become styled segments laid out
//! across one row. Pure: callers supply the variables, the time and the width.
use crate::config::Justify;
use crate::format::{Lists, PaneView, Vars, WindowFlags, pane_vars};
use crate::layout::WindowTab;
use std::time::{Duration, Instant};

use super::style::{Style, apply_directive};

/// A run of text drawn in one style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seg {
    pub text: String,
    pub style: Style,
}

impl Seg {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: Style::default(),
        }
    }

    fn width(&self) -> usize {
        self.text.chars().count()
    }
}

/// Width of a run of segments in cells (one per character).
pub fn segs_width(segs: &[Seg]) -> usize {
    segs.iter().map(Seg::width).sum()
}

/// Splits `text` at `#[...]` style directives. Directives set colours and attributes for the text
/// after them; `default` and `none` reset them. Unknown items (`align=`, `list=`) are ignored, and an
/// unclosed `#[` is kept as text.
pub fn parse_styled(text: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut style = Style::default();
    let mut rest = text;
    while let Some(start) = rest.find("#[") {
        let body = &rest[start + 2..];
        let Some(end) = body.find(']') else {
            break;
        };
        push_run(&mut segs, &rest[..start], &style);
        apply_directive(&mut style, &body[..end]);
        rest = &body[end + 1..];
    }
    push_run(&mut segs, rest, &style);
    segs
}

fn push_run(segs: &mut Vec<Seg>, text: &str, style: &Style) {
    if !text.is_empty() {
        segs.push(Seg {
            text: text.to_string(),
            style: style.clone(),
        });
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Expands the strftime subset tmux status lines use (`%H %I %M %S %p %d %e %m %b %a %y %Y %Z %%`)
/// for `unix_ms` at `utc_offset_secs`. `%Z` prints the offset (`+0100`) since the zone name is not
/// known. Unknown specifiers are kept as written.
pub fn strftime(fmt: &str, unix_ms: u64, utc_offset_secs: i64) -> String {
    let local = (unix_ms / 1000) as i64 + utc_offset_secs;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let weekday = (days + 4).rem_euclid(7) as usize;
    let hour12 = match hour % 12 {
        0 => 12,
        h => h,
    };
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('H') => out.push_str(&format!("{hour:02}")),
            Some('I') => out.push_str(&format!("{hour12:02}")),
            Some('M') => out.push_str(&format!("{minute:02}")),
            Some('S') => out.push_str(&format!("{second:02}")),
            Some('p') => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Some('d') => out.push_str(&format!("{day:02}")),
            Some('e') => out.push_str(&format!("{day:>2}")),
            Some('m') => out.push_str(&format!("{month:02}")),
            Some('b') => out.push_str(MONTHS[month as usize - 1]),
            Some('a') => out.push_str(DAYS[weekday]),
            Some('y') => out.push_str(&format!("{:02}", year.rem_euclid(100))),
            Some('Y') => out.push_str(&year.to_string()),
            Some('Z') => out.push_str(&utc_offset_text(utc_offset_secs)),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// `+HHMM` / `-HHMM` for a UTC offset in seconds.
fn utc_offset_text(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.abs() / 60;
    format!("{sign}{:02}{:02}", minutes / 60, minutes % 60)
}

/// Proleptic Gregorian `(year, month, day)` of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The tmux window flags of a tab, in tmux's order: `#` activity, `!` bell, `*` current, `Z`
/// zoomed (current window only), `-` last, `M` marked pane.
pub fn tab_flags(tab: &WindowTab, zoomed: bool) -> String {
    WindowFlags::of_tab(tab, zoomed).render()
}

/// Variables for a status format: the focused pane's (when known) with the window's own index,
/// name and flags. `window_base` is `base-index`, added to the daemon's 0-based index.
pub fn status_vars(
    session: &str,
    pane: Option<&PaneView>,
    tab: Option<&WindowTab>,
    zoomed: bool,
    window_base: usize,
) -> Vars {
    let mut vars = match pane {
        Some(view) => pane_vars(view),
        None => Vars::from([("session_name", session.to_string())]),
    };
    if let Some(tab) = tab {
        vars.insert("window_index", (tab.index + window_base).to_string());
        vars.insert("window_name", tab.name.clone());
        vars.insert("window_flags", tab_flags(tab, zoomed));
        vars.insert(
            "window_active",
            if tab.is_active { "1" } else { "0" }.to_string(),
        );
        for (name, on) in [
            ("window_activity_flag", tab.has_activity),
            ("window_bell_flag", tab.has_bell),
            ("window_silence_flag", tab.has_silence),
            ("window_last_flag", tab.is_last),
            ("window_marked_flag", tab.has_marked),
        ] {
            vars.insert(name, if on { "1" } else { "0" }.to_string());
        }
    }
    vars
}

/// The session a status line describes: its name, its times and how many clients are attached.
#[derive(Debug, Clone, Copy)]
pub struct SessionInfo<'a> {
    pub name: &'a str,
    pub times: crate::format::SessionTimes,
    pub attached: usize,
}

/// The lists the loop formats `#{W:}`, `#{P:}` and `#{S:}` iterate over: one entry per window (its
/// first pane's variables with the window's own), the panes of the current window, and the session.
pub fn status_lists(
    session: SessionInfo,
    tabs: &[WindowTab],
    panes: &[PaneView],
    zoomed: bool,
    window_base: usize,
) -> Lists {
    let windows = tabs
        .iter()
        .map(|tab| {
            let pane = panes.iter().find(|p| p.window == tab.index);
            status_vars(session.name, pane, Some(tab), zoomed, window_base)
        })
        .collect();
    let current = tabs.iter().find(|t| t.is_active).map(|t| t.index);
    let current_panes = panes
        .iter()
        .filter(|p| Some(p.window) == current)
        .map(pane_vars)
        .collect();
    let mut session_entry = crate::format::session_vars(session.name, session.times, session.attached);
    session_entry.insert("session_windows", tabs.len().to_string());
    let sessions = vec![session_entry];
    Lists {
        windows,
        panes: current_panes,
        sessions,
    }
}

/// Where a clickable region of the status row leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Window(usize),
    Pane(String),
}

/// One clickable item of the window list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub segs: Vec<Seg>,
    pub target: Option<Target>,
}

/// A laid-out status row: its segments and the columns `[start, end)` each target covers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub segs: Vec<Seg>,
    pub targets: Vec<(usize, usize, Target)>,
    /// First column after the left segment (`status-left`).
    pub left_end: usize,
    /// First column of the right segment (`status-right`).
    pub right_start: usize,
}

/// Lays out `left`, the window `items` (joined by `separator`) and `right` across `width` cells.
/// Free space goes after the items (left), around them (centre) or before them (right).
pub fn compose(
    width: usize,
    left: Vec<Seg>,
    items: Vec<Item>,
    separator: &str,
    right: Vec<Seg>,
    justify: Justify,
) -> Line {
    let sep_width = separator.chars().count();
    let items_width: usize = items.iter().map(|i| segs_width(&i.segs)).sum::<usize>()
        + sep_width * items.len().saturating_sub(1);
    let left_width = segs_width(&left);
    let right_width = segs_width(&right);
    let used = left_width + items_width + right_width;
    let free = width.saturating_sub(used);
    let (before, after) = match justify {
        Justify::Left => (0, free),
        Justify::Centre => (free / 2, free - free / 2),
        Justify::Right => (free, 0),
    };
    let mut segs = left;
    if before > 0 {
        segs.push(Seg::plain(" ".repeat(before)));
    }
    let mut col = left_width + before;
    let mut targets = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 && sep_width > 0 {
            segs.push(Seg::plain(separator));
            col += sep_width;
        }
        let width = segs_width(&item.segs);
        if let Some(target) = item.target {
            targets.push((col, col + width, target));
        }
        col += width;
        segs.extend(item.segs);
    }
    if after > 0 {
        segs.push(Seg::plain(" ".repeat(after)));
    }
    segs.extend(right);
    Line {
        segs,
        targets,
        left_end: left_width,
        right_start: width.saturating_sub(right_width),
    }
}

/// The first `max` cells of `segs`, keeping their styles.
pub fn truncate(segs: Vec<Seg>, max: usize) -> Vec<Seg> {
    let mut left = max;
    let mut out = Vec::new();
    for seg in segs {
        if left == 0 {
            break;
        }
        let width = seg.width();
        if width <= left {
            left -= width;
            out.push(seg);
        } else {
            let text: String = seg.text.chars().take(left).collect();
            out.push(Seg {
                text,
                style: seg.style,
            });
            left = 0;
        }
    }
    out
}

/// Screen rows for one frame. The panes fill `pane_h` rows from `pane_top`. The status block, when
/// shown, takes `status_h` rows from `status`, its first row; messages and prompts use `prompt`,
/// which is that first row when the block is shown and the bottom row (over the panes) when not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows {
    pub screen_h: u16,
    pub pane_top: u16,
    pub pane_h: u16,
    pub status: Option<u16>,
    pub status_h: u16,
    pub prompt: u16,
}

impl Rows {
    /// The status block's rows below its first: the `status-format` lines.
    pub fn extra_status_rows(&self) -> std::ops::Range<u16> {
        match self.status {
            Some(first) => first + 1..first + self.status_h,
            None => 0..0,
        }
    }
}

/// Lays out one frame. `status_lines` is the status block's height, 0 when it is hidden (capped at
/// the screen height); `top` puts the block above the panes instead of below them.
pub fn rows(screen_h: u16, status_lines: usize, top: bool) -> Rows {
    let status_h = status_lines.min(usize::from(screen_h)) as u16;
    if status_h == 0 {
        return Rows {
            screen_h,
            pane_top: 0,
            pane_h: screen_h,
            status: None,
            status_h: 0,
            prompt: screen_h.saturating_sub(1),
        };
    }
    let pane_h = screen_h - status_h;
    if top {
        Rows {
            screen_h,
            pane_top: status_h,
            pane_h,
            status: Some(0),
            status_h,
            prompt: 0,
        }
    } else {
        Rows {
            screen_h,
            pane_top: 0,
            pane_h,
            status: Some(pane_h),
            status_h,
            prompt: pane_h,
        }
    }
}

/// Whether a timed status redraw is due. `interval_s` is `status-interval`: 0 never redraws on a
/// timer, 1 redraws on every tick, more waits that many seconds after the last frame.
pub fn status_due(interval_s: u64, last: Option<Instant>, now: Instant) -> bool {
    match interval_s {
        0 => false,
        1 => true,
        n => last.is_none_or(|at| {
            now.saturating_duration_since(at) + Duration::from_millis(500) >= Duration::from_secs(n)
        }),
    }
}
