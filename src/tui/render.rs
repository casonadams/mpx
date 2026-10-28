use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, Write};

use super::status::agent_badge;
use crate::layout::{Divider, WindowTab, geometry::Rect};
use crate::vt::grid::Grid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionRange {
    Character {
        anchor_x: usize,
        anchor_abs_y: usize,
        cursor_x: usize,
        cursor_abs_y: usize,
    },
    Line {
        start_abs_y: usize,
        end_abs_y: usize,
    },
}

impl SelectionRange {
    pub fn contains_abs(&self, x: usize, abs_y: usize) -> bool {
        match self {
            Self::Line {
                start_abs_y,
                end_abs_y,
            } => {
                let min_y = (*start_abs_y).min(*end_abs_y);
                let max_y = (*start_abs_y).max(*end_abs_y);
                abs_y >= min_y && abs_y <= max_y
            }
            Self::Character {
                anchor_x,
                anchor_abs_y,
                cursor_x,
                cursor_abs_y,
            } => {
                let ((sx, sy), (ex, ey)) =
                    if (*anchor_abs_y, *anchor_x) <= (*cursor_abs_y, *cursor_x) {
                        ((*anchor_x, *anchor_abs_y), (*cursor_x, *cursor_abs_y))
                    } else {
                        ((*cursor_x, *cursor_abs_y), (*anchor_x, *anchor_abs_y))
                    };

                if abs_y < sy || abs_y > ey {
                    false
                } else if sy == ey {
                    x >= sx && x <= ex
                } else if abs_y == sy {
                    x >= sx
                } else if abs_y == ey {
                    x <= ex
                } else {
                    true
                }
            }
        }
    }
}
pub struct RenderPaneContext<'a> {
    pub rect: Rect,
    pub grid: &'a Grid,
    pub selection: Option<&'a SelectionRange>,
    pub scroll_offset: usize,
    pub badge: Option<&'a str>,
    pub dimmed: bool,
}

/// Inputs for the bottom status row.
pub struct StatusBar<'a> {
    pub session: &'a str,
    pub tabs: &'a [WindowTab],
    pub panes: &'a [(String, Rect, bool)],
    pub prefix_active: bool,
    pub waiting_agents: usize,
    pub width: u16,
    pub y: u16,
}

fn divider_char_color(x: u16, y: u16, rect: Option<Rect>) -> Color {
    if let Some(r) = rect {
        let is_left = x + 1 == r.x && y + 1 >= r.y && y <= r.y + r.height;
        let is_right = x == r.x + r.width && y + 1 >= r.y && y <= r.y + r.height;
        let is_top = y + 1 == r.y && x + 1 >= r.x && x <= r.x + r.width;
        let is_bottom = y == r.y + r.height && x + 1 >= r.x && x <= r.x + r.width;
        if is_left || is_right || is_top || is_bottom {
            return Color::Green;
        }
    }
    Color::DarkGrey
}

fn junction_char(up: bool, down: bool, left: bool, right: bool) -> &'static str {
    match (up, down, left, right) {
        (true, true, true, true) => "┼",
        (true, true, true, false) => "┤",
        (true, true, false, true) => "├",
        (false, true, true, true) => "┬",
        (true, false, true, true) => "┴",
        (false, true, false, true) => "┌",
        (false, true, true, false) => "┐",
        (true, false, false, true) => "└",
        (true, false, true, false) => "┘",
        (true, true, false, false) => "│",
        (false, false, true, true) => "─",
        _ => "┼",
    }
}

fn get_divider_char(div: &Divider, x: u16, y: u16, all: &[Divider]) -> &'static str {
    let has_v = all
        .iter()
        .any(|d| d.is_vertical && d.x == x && (y + 1 >= d.y && y <= d.y + d.length));
    let has_h = all
        .iter()
        .any(|d| !d.is_vertical && d.y == y && (x + 1 >= d.x && x <= d.x + d.length));

    if has_v && has_h {
        let up = all.iter().any(|d| d.is_vertical && d.x == x && d.y < y);
        let down = all
            .iter()
            .any(|d| d.is_vertical && d.x == x && (d.y == y + 1 || d.y + d.length > y));
        let left = all.iter().any(|d| !d.is_vertical && d.y == y && d.x < x);
        let right = all
            .iter()
            .any(|d| !d.is_vertical && d.y == y && (d.x == x + 1 || d.x + d.length > x));
        junction_char(up, down, left, right)
    } else if div.is_vertical {
        "│"
    } else {
        "─"
    }
}

fn render_vertical_divider<W: Write>(
    w: &mut W,
    div: &Divider,
    all_dividers: &[Divider],
    active_rect: Option<Rect>,
    cur_color: &mut Option<Color>,
) -> io::Result<()> {
    for y in div.y..(div.y + div.length) {
        let color = divider_char_color(div.x, y, active_rect);
        if *cur_color != Some(color) {
            execute!(w, SetForegroundColor(color))?;
            *cur_color = Some(color);
        }
        let ch = get_divider_char(div, div.x, y, all_dividers);
        execute!(w, MoveTo(div.x, y), Print(ch))?;
    }
    Ok(())
}

fn render_horizontal_divider<W: Write>(
    w: &mut W,
    div: &Divider,
    all_dividers: &[Divider],
    active_rect: Option<Rect>,
    cur_color: &mut Option<Color>,
) -> io::Result<()> {
    for x in div.x..(div.x + div.length) {
        let color = divider_char_color(x, div.y, active_rect);
        if *cur_color != Some(color) {
            execute!(w, SetForegroundColor(color))?;
            *cur_color = Some(color);
        }
        let ch = get_divider_char(div, x, div.y, all_dividers);
        execute!(w, MoveTo(x, div.y), Print(ch))?;
    }
    Ok(())
}

pub struct Compositor;

impl Compositor {
    /// Renders terminal pane content directly into its bounding box with optional visual selection, scroll offset, and badge
    /// Renders terminal pane content directly into its bounding box
    pub fn draw_pane_content<W: Write>(w: &mut W, ctx: RenderPaneContext<'_>) -> io::Result<()> {
        if ctx.rect.width == 0 || ctx.rect.height == 0 {
            return Ok(());
        }

        let badge_chars = ctx.badge.map(|b| b.chars().collect::<Vec<char>>());
        let badge_info = badge_chars.as_ref().and_then(|chars| {
            if (ctx.rect.width as usize) > chars.len() + 1 {
                let start_col = (ctx.rect.width as usize) - chars.len() - 1;
                Some((start_col, chars.as_slice()))
            } else {
                None
            }
        });

        for row in 0..ctx.rect.height as usize {
            render_row_cells(w, row, &ctx, badge_info)?;
        }

        execute!(w, ResetColor)?;
        Ok(())
    }

    /// Renders clean tmux-style single-line dividers between panes with green active pane highlighting
    pub fn draw_dividers<W: Write>(
        w: &mut W,
        dividers: &[Divider],
        active_rect: Option<Rect>,
    ) -> io::Result<()> {
        let mut cur_color: Option<Color> = None;
        for div in dividers {
            if div.is_vertical {
                render_vertical_divider(w, div, dividers, active_rect, &mut cur_color)?;
            } else {
                render_horizontal_divider(w, div, dividers, active_rect, &mut cur_color)?;
            }
        }
        execute!(w, ResetColor)?;
        Ok(())
    }

    /// Renders the bottom status row: window tabs (or pane names), agent badge, prefix badge, clock.
    pub fn draw_status_bar<W: Write>(w: &mut W, bar: &StatusBar<'_>) -> io::Result<()> {
        let left = status_left(bar);
        let right = status_right(bar, &get_current_time());
        let total_chars = left.chars().count() + right.chars().count();
        let pad_len = (bar.width as usize).saturating_sub(total_chars + 1);
        execute!(
            w,
            MoveTo(0, bar.y),
            SetBackgroundColor(Color::DarkGreen),
            SetForegroundColor(Color::Black),
            Print(&left),
            Print(" ".repeat(pad_len)),
            Print(&right),
            ResetColor
        )
    }

    /// Renders a one-line command row (`:` prompt, confirmation question or message) across the status row.
    pub fn draw_command_line<W: Write>(
        w: &mut W,
        text: &str,
        y: u16,
        width: u16,
    ) -> io::Result<()> {
        let line: String = text.chars().take(width as usize).collect();
        let pad = (width as usize).saturating_sub(line.chars().count());
        execute!(
            w,
            MoveTo(0, y),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::White),
            Print(line),
            Print(" ".repeat(pad)),
            ResetColor
        )
    }
    /// Floating popup listing the prefix shortcuts, anchored bottom-right above the status row.
    pub fn draw_shortcut_hud<W: Write>(w: &mut W, screen_w: u16, screen_h: u16) -> io::Result<()> {
        let box_w: u16 = 30;
        let box_h: u16 = (HUD_ITEMS.len() as u16 + 2).min(screen_h.saturating_sub(2));
        if box_w >= screen_w || box_h < 4 {
            return Ok(());
        }
        let box_x = screen_w.saturating_sub(box_w + 2);
        let box_y = screen_h.saturating_sub(box_h + 1);
        draw_hud_header(w, box_x, box_y, box_w)?;
        for (i, (key, desc)) in HUD_ITEMS.iter().take((box_h - 2) as usize).enumerate() {
            draw_hud_row(w, (box_x, box_y + 1 + i as u16), box_w, key, desc)?;
        }
        draw_hud_footer(w, box_x, box_y + box_h - 1, box_w)
    }

    pub fn draw_copy_mode_badge<W: Write>(
        w: &mut W,
        rect: Rect,
        mode_label: &str,
        scroll_offset: usize,
        total_history: usize,
    ) -> io::Result<()> {
        let text = format!(
            " [{} | -{}/{} lines | y: yank | q: exit] ",
            mode_label, scroll_offset, total_history
        );
        let badge_x = (rect.x + rect.width).saturating_sub(text.len() as u16 + 1);
        if badge_x >= rect.x {
            execute!(
                w,
                MoveTo(badge_x, rect.y),
                SetBackgroundColor(Color::Rgb {
                    r: 255,
                    g: 140,
                    b: 0
                }),
                SetForegroundColor(Color::Black),
                Print(text),
                ResetColor
            )?;
        }
        Ok(())
    }

    /// Emits DEC 2026 Begin Synchronized Update sequence to eliminate tearing and cursor jitter
    pub fn begin_sync<W: Write>(w: &mut W) -> io::Result<()> {
        w.write_all(b"\x1b[?2026h")
    }

    /// Emits DEC 2026 End Synchronized Update sequence to flush complete frame atomically
    pub fn end_sync<W: Write>(w: &mut W) -> io::Result<()> {
        w.write_all(b"\x1b[?2026l")
    }

    /// Places the live terminal hardware cursor at the focused pane's active position
    pub fn position_cursor<W: Write>(
        w: &mut W,
        pane_rect: Rect,
        cursor_x: usize,
        cursor_y: usize,
        visible: bool,
    ) -> io::Result<()> {
        if !visible {
            execute!(w, Hide)?;
            return Ok(());
        }

        let cx = (pane_rect.x as usize + cursor_x)
            .min((pane_rect.x + pane_rect.width).saturating_sub(1) as usize)
            as u16;
        let cy = (pane_rect.y as usize + cursor_y)
            .min((pane_rect.y + pane_rect.height).saturating_sub(1) as usize)
            as u16;

        execute!(w, MoveTo(cx, cy), Show)?;
        Ok(())
    }
}

/// Left side of the status row: `[session] ` followed by window tabs, or pane names when there are none.
pub fn status_left(bar: &StatusBar<'_>) -> String {
    let mut left = format!("[{}] ", bar.session);
    if bar.tabs.is_empty() {
        for (i, (name, _, is_focused)) in bar.panes.iter().enumerate() {
            left.push_str(&format!("{}:{}{} ", i, name, if *is_focused { "*" } else { "-" }));
        }
    } else {
        for tab in bar.tabs {
            left.push_str(&format!("{}:{}{} ", tab.index, tab.name, if tab.is_active { "*" } else { "-" }));
        }
    }
    left
}

/// Right side of the status row: waiting-agent badge, prefix indicator and `clock`.
pub fn status_right(bar: &StatusBar<'_>, clock: &str) -> String {
    let mut right = String::new();
    if let Some(badge) = agent_badge(bar.waiting_agents) {
        right.push_str(&format!(" {badge} "));
    }
    right.push_str(if bar.prefix_active { " [PREFIX] " } else { " " });
    right.push_str(clock);
    right
}

const HUD_ITEMS: &[(&str, &str)] = &[
    ("c", "new window"),
    ("p", "previous window"),
    ("n", "next window"),
    ("%", "split vertical"),
    ("\"", "split horizontal"),
    ("h", "focus left"),
    ("j", "focus down"),
    ("k", "focus up"),
    ("l", "focus right"),
    ("o", "next pane"),
    ("z", "toggle zoom"),
    ("[", "copy mode"),
    ("x", "kill pane"),
    ("d", "detach"),
    ("?", "help"),
];

fn draw_hud_header<W: Write>(w: &mut W, x: u16, y: u16, box_w: u16) -> io::Result<()> {
    execute!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("╭"),
        Print("─".repeat(box_w.saturating_sub(2) as usize)),
        Print("╮"),
        ResetColor
    )
}

fn draw_hud_row<W: Write>(
    w: &mut W,
    (x, y): (u16, u16),
    box_w: u16,
    key: &str,
    desc: &str,
) -> io::Result<()> {
    execute!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("│ "),
        SetForegroundColor(Color::Yellow),
        Print(format!("{:<2}", key)),
        SetForegroundColor(Color::DarkGrey),
        Print(" → "),
        SetForegroundColor(Color::Cyan),
        Print(format!("{:<18}", desc)),
    )?;
    let used = 2 + 2 + 3 + 18;
    let pad = (box_w as usize).saturating_sub(used + 1);
    execute!(
        w,
        Print(" ".repeat(pad)),
        SetForegroundColor(Color::DarkCyan),
        Print("│"),
        ResetColor
    )
}

fn draw_hud_footer<W: Write>(w: &mut W, x: u16, y: u16, box_w: u16) -> io::Result<()> {
    let footer_text = " ESC close ";
    let dash_count = (box_w as usize).saturating_sub(footer_text.len() + 2);
    execute!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("╰"),
        Print("─".repeat(dash_count)),
        SetForegroundColor(Color::Yellow),
        Print(" ESC "),
        SetForegroundColor(Color::DarkGrey),
        Print("close"),
        SetForegroundColor(Color::DarkCyan),
        Print(" ╯"),
        ResetColor
    )
}

fn cell_content(
    col: usize,
    row: usize,
    abs_row: usize,
    ctx: &RenderPaneContext<'_>,
    badge_info: Option<(usize, &[char])>,
) -> (char, Color, Color) {
    if row == 0
        && let Some((b_start, b_chars)) = badge_info
        && col >= b_start
        && col < b_start + b_chars.len()
    {
        return (
            b_chars[col - b_start],
            Color::Black,
            Color::Rgb {
                r: 255,
                g: 140,
                b: 0,
            },
        );
    }

    let cell = ctx
        .grid
        .get_cell_scrolled(col, row, ctx.scroll_offset)
        .copied()
        .unwrap_or_default();
    if ctx.selection.is_some_and(|s| s.contains_abs(col, abs_row)) {
        (
            cell.c,
            Color::Black,
            Color::Rgb {
                r: 255,
                g: 140,
                b: 0,
            },
        )
    } else {
        let fg = cell.style.fg.map(convert_color).unwrap_or(Color::Reset);
        let bg = cell.style.bg.map(convert_color).unwrap_or(Color::Reset);
        if ctx.dimmed {
            (cell.c, dim_color(fg), bg)
        } else {
            (cell.c, fg, bg)
        }
    }
}

fn render_row_cells<W: Write>(
    w: &mut W,
    row: usize,
    ctx: &RenderPaneContext<'_>,
    badge_info: Option<(usize, &[char])>,
) -> io::Result<()> {
    let abs_row = ctx.grid.abs_line(row, ctx.scroll_offset);
    let screen_y = ctx.rect.y + row as u16;
    execute!(w, MoveTo(ctx.rect.x, screen_y))?;
    let mut cur_fg: Option<Color> = None;
    let mut cur_bg: Option<Color> = None;

    for col in 0..ctx.rect.width as usize {
        let (ch, fg, bg) = cell_content(col, row, abs_row, ctx, badge_info);
        if cur_fg != Some(fg) {
            execute!(w, SetForegroundColor(fg))?;
            cur_fg = Some(fg);
        }
        if cur_bg != Some(bg) {
            execute!(w, SetBackgroundColor(bg))?;
            cur_bg = Some(bg);
        }
        execute!(w, Print(ch))?;
    }
    execute!(w, ResetColor)?;
    Ok(())
}

fn convert_color(c: crate::vt::grid::Color) -> Color {
    match c {
        crate::vt::grid::Color::Black => Color::Black,
        crate::vt::grid::Color::Red => Color::Red,
        crate::vt::grid::Color::Green => Color::Green,
        crate::vt::grid::Color::Yellow => Color::Yellow,
        crate::vt::grid::Color::Blue => Color::Blue,
        crate::vt::grid::Color::Magenta => Color::Magenta,
        crate::vt::grid::Color::Cyan => Color::Cyan,
        crate::vt::grid::Color::White => Color::White,
        crate::vt::grid::Color::BrightBlack => Color::DarkGrey,
        crate::vt::grid::Color::BrightRed => Color::Red,
        crate::vt::grid::Color::BrightGreen => Color::Green,
        crate::vt::grid::Color::BrightYellow => Color::Yellow,
        crate::vt::grid::Color::BrightBlue => Color::Blue,
        crate::vt::grid::Color::BrightMagenta => Color::Magenta,
        crate::vt::grid::Color::BrightCyan => Color::Cyan,
        crate::vt::grid::Color::BrightWhite => Color::White,
        crate::vt::grid::Color::Indexed(i) => Color::AnsiValue(i),
        crate::vt::grid::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

fn dim_color(c: Color) -> Color {
    match c {
        Color::Reset | Color::White | Color::Grey => Color::DarkGrey,
        Color::Rgb { r, g, b } => Color::Rgb {
            r: (r as u16 * 140 / 255) as u8,
            g: (g as u16 * 140 / 255) as u8,
            b: (b as u16 * 140 / 255) as u8,
        },
        Color::Red => Color::DarkRed,
        Color::Green => Color::DarkGreen,
        Color::Yellow => Color::DarkYellow,
        Color::Blue => Color::DarkBlue,
        Color::Magenta => Color::DarkMagenta,
        Color::Cyan => Color::DarkCyan,
        other => other,
    }
}

fn get_current_time() -> String {
    // 12-hour %I:%M:%S matching tmux status-right
    let total_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours_24 = (total_secs / 3600) % 24;
    let hours_12 = match hours_24 % 12 {
        0 => 12,
        h => h,
    };
    format!(" {:02}:{:02}:{:02}", hours_12, mins, secs)
}

pub fn copy_to_clipboard(text: &str) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    }

    // Fallback: OSC 52 ANSI clipboard sequence supported by all modern terminals
    let b64 = base64_encode(text.as_bytes());
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", b64);
    let _ = out.flush();
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
