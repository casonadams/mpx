use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, Write};

use super::canvas::Canvas;
use super::status::agent_badge;
use crate::layout::{Divider, WindowTab, geometry::Rect};
use crate::vt::grid::Grid;
use crate::vt::{Cell, CellStyle, Color as VtColor};

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

fn divider_color(x: u16, y: u16, rect: Option<Rect>) -> VtColor {
    if let Some(r) = rect {
        let is_left = x + 1 == r.x && y + 1 >= r.y && y <= r.y + r.height;
        let is_right = x == r.x + r.width && y + 1 >= r.y && y <= r.y + r.height;
        let is_top = y + 1 == r.y && x + 1 >= r.x && x <= r.x + r.width;
        let is_bottom = y == r.y + r.height && x + 1 >= r.x && x <= r.x + r.width;
        if is_left || is_right || is_top || is_bottom {
            return VtColor::BrightGreen;
        }
    }
    VtColor::BrightBlack
}

fn divider_cell(div: &Divider, (x, y): (u16, u16), all: &[Divider], active: Option<Rect>) -> Cell {
    Cell {
        c: get_divider_char(div, x, y, all),
        style: CellStyle {
            fg: Some(divider_color(x, y, active)),
            ..CellStyle::default()
        },
    }
}

fn junction_char(up: bool, down: bool, left: bool, right: bool) -> char {
    match (up, down, left, right) {
        (true, true, true, true) => '┼',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (true, true, false, false) => '│',
        (false, false, true, true) => '─',
        _ => '┼',
    }
}

fn get_divider_char(div: &Divider, x: u16, y: u16, all: &[Divider]) -> char {
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
        '│'
    } else {
        '─'
    }
}

/// Cells a divider covers, top to bottom or left to right.
fn divider_cells(div: &Divider) -> impl Iterator<Item = (u16, u16)> + '_ {
    (0..div.length).map(move |i| {
        if div.is_vertical {
            (div.x, div.y + i)
        } else {
            (div.x + i, div.y)
        }
    })
}

pub struct Compositor;

impl Compositor {
    /// Draws a pane's cells (with selection, scroll offset and badge) into its rect on `canvas`.
    pub fn draw_pane_content<W: Write>(
        canvas: &mut Canvas,
        w: &mut W,
        ctx: RenderPaneContext<'_>,
    ) -> io::Result<()> {
        let badge_chars = ctx.badge.map(|b| b.chars().collect::<Vec<char>>());
        let badge_info = badge_chars.as_ref().and_then(|chars| {
            if (ctx.rect.width as usize) > chars.len() + 1 {
                let start_col = (ctx.rect.width as usize) - chars.len() - 1;
                Some((start_col, chars.as_slice()))
            } else {
                None
            }
        });
        let mut cells = Vec::with_capacity(ctx.rect.width as usize);
        for row in 0..ctx.rect.height as usize {
            let abs_row = ctx.grid.abs_line(row, ctx.scroll_offset);
            cells.clear();
            cells.extend(
                (0..ctx.rect.width as usize)
                    .map(|col| cell_content(col, row, abs_row, &ctx, badge_info)),
            );
            canvas.put_row(w, ctx.rect.x, ctx.rect.y + row as u16, &cells)?;
        }
        Ok(())
    }

    /// Draws tmux-style single-line dividers on `canvas`, highlighting the active pane's edges.
    pub fn draw_dividers<W: Write>(
        canvas: &mut Canvas,
        w: &mut W,
        dividers: &[Divider],
        active_rect: Option<Rect>,
    ) -> io::Result<()> {
        for div in dividers {
            for pos in divider_cells(div) {
                let cell = divider_cell(div, pos, dividers, active_rect);
                canvas.put(w, pos.0, pos.1, cell)?;
            }
        }
        Ok(())
    }

    /// Renders the bottom status row: window tabs (or pane names), agent badge, prefix badge, clock.
    pub fn draw_status_bar<W: Write>(w: &mut W, bar: &StatusBar<'_>) -> io::Result<()> {
        let left = status_left(bar);
        let right = status_right(bar, &get_current_time());
        let total_chars = left.chars().count() + right.chars().count();
        let pad_len = (bar.width as usize).saturating_sub(total_chars + 1);
        queue!(
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
        queue!(
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
        let Some(area) = hud_rect(screen_w, screen_h) else {
            return Ok(());
        };
        let (box_x, box_y, box_w, box_h) = (area.x, area.y, area.width, area.height);
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
            queue!(
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
            queue!(w, Hide)?;
            return Ok(());
        }

        let cx = (pane_rect.x as usize + cursor_x)
            .min((pane_rect.x + pane_rect.width).saturating_sub(1) as usize)
            as u16;
        let cy = (pane_rect.y as usize + cursor_y)
            .min((pane_rect.y + pane_rect.height).saturating_sub(1) as usize)
            as u16;

        queue!(w, MoveTo(cx, cy), Show)?;
        Ok(())
    }
}

/// Left side of the status row: `[session] ` followed by window tabs, or pane names when there are none.
pub fn status_left(bar: &StatusBar<'_>) -> String {
    let mut left = format!("[{}] ", bar.session);
    if bar.tabs.is_empty() {
        for (i, (name, _, is_focused)) in bar.panes.iter().enumerate() {
            left.push_str(&format!(
                "{}:{}{} ",
                i,
                name,
                if *is_focused { "*" } else { "-" }
            ));
        }
    } else {
        for tab in bar.tabs {
            left.push_str(&format!(
                "{}:{}{} ",
                tab.index,
                tab.name,
                if tab.is_active { "*" } else { "-" }
            ));
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

/// Where the prefix HUD sits: bottom-right above the status row; `None` when the screen is too small.
pub fn hud_rect(screen_w: u16, screen_h: u16) -> Option<Rect> {
    let box_w: u16 = 30;
    let box_h: u16 = (HUD_ITEMS.len() as u16 + 2).min(screen_h.saturating_sub(2));
    if box_w >= screen_w || box_h < 4 {
        return None;
    }
    let box_x = screen_w.saturating_sub(box_w + 2);
    let box_y = screen_h.saturating_sub(box_h + 1);
    Some(Rect::new(box_x, box_y, box_w, box_h))
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
    queue!(
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
    queue!(
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
    queue!(
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
    queue!(
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
) -> Cell {
    if row == 0
        && let Some((b_start, b_chars)) = badge_info
        && col >= b_start
        && col < b_start + b_chars.len()
    {
        return highlighted(b_chars[col - b_start]);
    }
    let cell = ctx
        .grid
        .get_cell_scrolled(col, row, ctx.scroll_offset)
        .copied()
        .unwrap_or_default();
    if ctx.selection.is_some_and(|s| s.contains_abs(col, abs_row)) {
        highlighted(cell.c)
    } else if ctx.dimmed {
        Cell {
            c: cell.c,
            style: CellStyle {
                fg: Some(dim(cell.style.fg)),
                ..cell.style
            },
        }
    } else {
        cell
    }
}

/// Black on orange, for badges and copy-mode selection.
fn highlighted(c: char) -> Cell {
    Cell {
        c,
        style: CellStyle {
            fg: Some(VtColor::Black),
            bg: Some(VtColor::Rgb(255, 140, 0)),
            ..CellStyle::default()
        },
    }
}

/// Foreground of an unfocused pane: default and white turn grey, bright colors their normal shade.
fn dim(fg: Option<VtColor>) -> VtColor {
    match fg {
        None | Some(VtColor::White | VtColor::BrightWhite) => VtColor::BrightBlack,
        Some(VtColor::Rgb(r, g, b)) => VtColor::Rgb(
            (r as u16 * 140 / 255) as u8,
            (g as u16 * 140 / 255) as u8,
            (b as u16 * 140 / 255) as u8,
        ),
        Some(VtColor::BrightRed) => VtColor::Red,
        Some(VtColor::BrightGreen) => VtColor::Green,
        Some(VtColor::BrightYellow) => VtColor::Yellow,
        Some(VtColor::BrightBlue) => VtColor::Blue,
        Some(VtColor::BrightMagenta) => VtColor::Magenta,
        Some(VtColor::BrightCyan) => VtColor::Cyan,
        Some(other) => other,
    }
}

fn get_current_time() -> String {
    let now = crate::node::now_ms();
    format!(
        " {}",
        super::status::clock_12h(now, super::status::local_utc_offset(now))
    )
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
