use std::io::{self, Write};

use crossterm::{cursor::MoveTo, queue};

use super::Compositor;
use crate::format::WindowFlags;
use crate::layout::{WindowTab, geometry::Rect};
use crate::vt::Color as VtColor;

/// Inputs for the bottom status row.
pub struct StatusBar<'a> {
    pub session: &'a str,
    pub tabs: &'a [WindowTab],
    pub panes: &'a [(String, Rect, bool)],
    pub prefix_active: bool,
    pub width: u16,
    pub y: u16,
    pub bg: Option<VtColor>,
    pub fg: Option<VtColor>,
    pub zoomed: bool,
    pub copy_pane: Option<&'a str>,
    pub marked_pane: Option<String>,
}

/// A window tab of the built-in look: `index:name` and tmux's flags, the index counted from `base`.
pub fn tab_text(tab: &WindowTab, zoomed: bool, base: usize) -> String {
    let flags = if tab.is_active {
        WindowFlags {
            current: true,
            zoomed,
            marked: tab.has_marked,
            ..Default::default()
        }
        .render()
    } else {
        let mut flags = String::new();
        if tab.is_last {
            flags.push('-');
        }
        if tab.has_bell {
            flags.push('!');
        }
        if tab.has_silence {
            flags.push('~');
        }
        if tab.has_activity {
            flags.push('#');
        }
        if tab.has_marked {
            flags.push('M');
        }
        flags
    };
    format!("{}:{}{} ", tab.index + base, tab.name, flags)
}

/// Formats a single window tab with agent badges and tmux flags.
pub fn format_tab(tab: &WindowTab, zoomed: bool) -> String {
    tab_text(tab, zoomed, 0)
}

/// Left side of the status row: `[session] ` followed by window tabs, or pane names when there are none.
pub fn status_left(bar: &StatusBar<'_>) -> String {
    let mut left = format!("[{}] ", bar.session);
    if bar.tabs.is_empty() {
        for (i, (name, _, is_focused)) in bar.panes.iter().enumerate() {
            let mut flag = match (*is_focused, bar.zoomed) {
                (true, true) => "*Z".to_string(),
                (true, false) => "*".to_string(),
                (false, _) => "-".to_string(),
            };
            if bar.marked_pane.as_deref() == Some(name) {
                flag.push('M');
            }
            left.push_str(&format!("{}:{}{} ", i, name, flag));
        }
    } else {
        for tab in bar.tabs {
            left.push_str(&format_tab(tab, bar.zoomed));
        }
    }
    left
}

/// Right side of the status row: waiting-agent badge, prefix indicator and `clock`.
pub fn status_right(bar: &StatusBar<'_>, clock: &str) -> String {
    let mut right = String::new();
    if let Some(pane) = bar.copy_pane {
        right.push_str(&format!("[{pane}] "));
    }
    if bar.zoomed {
        right.push_str("Z ");
    }
    right.push_str(clock);
    right
}

pub fn get_current_time() -> String {
    let now = crate::node::now_ms();
    format!(
        " {}",
        crate::tui::status::clock_12h(now, crate::tui::status::local_utc_offset(now))
    )
}

impl Compositor {
    /// Renders the bottom status row across its full width: window tabs (or pane names), agent
    /// badge, prefix badge, clock. Autowrap is off, so the last column can be painted safely.
    pub fn draw_status_bar<W: Write>(w: &mut W, bar: &StatusBar<'_>) -> io::Result<()> {
        let line = crate::tui::statusline::compose(
            bar.width as usize,
            vec![crate::tui::statusline::Seg::plain(status_left(bar))],
            Vec::new(),
            "",
            vec![crate::tui::statusline::Seg::plain(status_right(
                bar,
                &get_current_time(),
            ))],
            crate::config::Justify::Left,
        );
        draw_status_segments(w, &line.segs, bar.y, bar.bg, bar.fg)
    }
    /// Renders a one-line command row (`:` prompt, confirmation question or message) across the
    /// status row, drawn in `style` over white on black.
    pub fn draw_command_line<W: Write>(
        w: &mut W,
        text: &str,
        y: u16,
        width: u16,
        style: &crate::tui::style::Style,
    ) -> io::Result<()> {
        let line: String = text.chars().take(width as usize).collect();
        let pad = (width as usize).saturating_sub(line.chars().count());
        queue!(w, MoveTo(0, y))?;
        write_status_run(
            w,
            style,
            VtColor::White,
            VtColor::Black,
            &format!("{line}{}", " ".repeat(pad)),
        )?;
        write!(w, "\x1b[0m")
    }
}

/// Writes one status row of styled segments at `y`; colours a segment does not set are `bg`/`fg`.
/// Neighbouring segments in the same style share one colour sequence.
pub fn draw_status_segments<W: Write>(
    w: &mut W,
    segs: &[crate::tui::statusline::Seg],
    y: u16,
    bg: Option<VtColor>,
    fg: Option<VtColor>,
) -> io::Result<()> {
    let (bg, fg) = (bg.unwrap_or(VtColor::Green), fg.unwrap_or(VtColor::Black));
    queue!(w, MoveTo(0, y))?;
    let mut i = 0;
    while i < segs.len() {
        let style = &segs[i].style;
        let mut end = i;
        let mut text = String::new();
        while end < segs.len() && segs[end].style == *style {
            text.push_str(&segs[end].text);
            end += 1;
        }
        write_status_run(w, style, fg, bg, &text)?;
        i = end;
    }
    write!(w, "\x1b[0m")
}

fn write_status_run<W: Write>(
    w: &mut W,
    style: &crate::tui::style::Style,
    fg: VtColor,
    bg: VtColor,
    text: &str,
) -> io::Result<()> {
    let (fg, bg) = (style.fg.unwrap_or(fg), style.bg.unwrap_or(bg));
    let (fg, bg) = if style.reverse { (bg, fg) } else { (fg, bg) };
    write!(w, "\x1b[0")?;
    write_vt_color(w, fg, 30)?;
    write_vt_color(w, bg, 40)?;
    write!(w, "m")?;
    if style.bold {
        write!(w, "\x1b[1m")?;
    }
    if style.dim {
        write!(w, "\x1b[2m")?;
    }
    if style.underscore {
        write!(w, "\x1b[4m")?;
    }
    if style.italics {
        write!(w, "\x1b[3m")?;
    }
    write!(w, "{text}")
}

fn write_vt_color<W: Write>(out: &mut W, color: VtColor, base: u8) -> io::Result<()> {
    match color {
        VtColor::Indexed(i) => write!(out, ";{};5;{i}", base + 8),
        VtColor::Rgb(r, g, b) => write!(out, ";{};2;{r};{g};{b}", base + 8),
        VtColor::Black => write!(out, ";{base}"),
        VtColor::Red => write!(out, ";{}", base + 1),
        VtColor::Green => write!(out, ";{}", base + 2),
        VtColor::Yellow => write!(out, ";{}", base + 3),
        VtColor::Blue => write!(out, ";{}", base + 4),
        VtColor::Magenta => write!(out, ";{}", base + 5),
        VtColor::Cyan => write!(out, ";{}", base + 6),
        VtColor::White => write!(out, ";{}", base + 7),
        VtColor::BrightBlack => write!(out, ";{}", base + 60),
        VtColor::BrightRed => write!(out, ";{}", base + 61),
        VtColor::BrightGreen => write!(out, ";{}", base + 62),
        VtColor::BrightYellow => write!(out, ";{}", base + 63),
        VtColor::BrightBlue => write!(out, ";{}", base + 64),
        VtColor::BrightMagenta => write!(out, ";{}", base + 65),
        VtColor::BrightCyan => write!(out, ";{}", base + 66),
        VtColor::BrightWhite => write!(out, ";{}", base + 67),
    }
}
