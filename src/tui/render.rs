use std::io::{self, Write};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use crate::layout::{geometry::Rect, Divider, WindowTab};
use crate::vt::grid::{Cell, Grid};

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
                let ((sx, sy), (ex, ey)) = if (*anchor_abs_y, *anchor_x) <= (*cursor_abs_y, *cursor_x) {
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
}


pub struct Compositor;

impl Compositor {
    /// Renders terminal pane content directly into its bounding box with optional visual selection, scroll offset, and badge
    /// Renders terminal pane content directly into its bounding box
    pub fn draw_pane_content<W: Write>(
        w: &mut W,
        ctx: RenderPaneContext<'_>,
    ) -> io::Result<()> {
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

    /// Renders clean tmux-style single-line dividers between panes
    pub fn draw_dividers<W: Write>(
        w: &mut W,
        dividers: &[Divider],
    ) -> io::Result<()> {
        execute!(w, SetForegroundColor(Color::DarkGrey))?;
        for div in dividers {
            if div.is_vertical {
                for y in div.y..(div.y + div.length) {
                    execute!(w, MoveTo(div.x, y), Print("│"))?;
                }
            } else {
                execute!(w, MoveTo(div.x, div.y))?;
                for _ in 0..div.length {
                    execute!(w, Print("─"))?;
                }
            }
        }
        execute!(w, ResetColor)?;
        Ok(())
    }

    /// Renders a clean tmux-style status line at the bottom
    pub fn draw_status_bar<W: Write>(
        w: &mut W,
        session: &str,
        tabs: &[WindowTab],
        panes: &[(String, Rect, bool)],
        leader_active: bool,
        width: u16,
        y: u16,
    ) -> io::Result<()> {
        execute!(
            w,
            MoveTo(0, y),
            SetBackgroundColor(Color::DarkGreen),
            SetForegroundColor(Color::Black)
        )?;

        let mut left = format!("[{}] ", session);
        if !tabs.is_empty() {
            for tab in tabs {
                let mark = if tab.is_active { "*" } else { "-" };
                left.push_str(&format!("{}:{}{} ", tab.index, tab.name, mark));
            }
        } else {
            for (i, (name, _, is_focused)) in panes.iter().enumerate() {
                let active_mark = if *is_focused { "*" } else { "-" };
                left.push_str(&format!("{}:{}{} ", i, name, active_mark));
            }
        }

        let prefix_badge = if leader_active { " [PREFIX] " } else { " " };
        let right = format!("{}{}", prefix_badge, get_current_time());

        let total_chars = left.chars().count() + right.chars().count();
        let pad_len = (width as usize).saturating_sub(total_chars);

        execute!(
            w,
            Print(&left),
            Print(" ".repeat(pad_len)),
            Print(&right),
            ResetColor
        )?;

        Ok(())
    }
    /// Floating modern vertical popup menu anchored in bottom-right corner (Neovim style)
    pub fn draw_shortcut_hud<W: Write>(w: &mut W, screen_w: u16, screen_h: u16) -> io::Result<()> {
        let items: &[(&str, &str)] = &[
            ("c", "new window"),
            ("p", "previous window"),
            ("n", "next window"),
            ("%", "split vertical"),
            ("\"", "split horizontal"),
            ("h", "focus left"),
            ("j", "focus down"),
            ("k", "focus up"),
            ("l", "focus right"),
            ("o", "rotate pane"),
            ("z", "toggle zoom"),
            ("[", "copy mode"),
            ("x", "kill pane"),
            ("d", "detach"),
        ];

        let box_w: u16 = 30;
        let box_h: u16 = (items.len() as u16 + 2).min(screen_h.saturating_sub(2));
        if box_w >= screen_w || box_h < 4 {
            return Ok(());
        }

        let box_x = screen_w.saturating_sub(box_w + 2);
        let box_y = screen_h.saturating_sub(box_h + 1); // 1 line above bottom statusline

        // Header (rounded border)
        execute!(
            w,
            MoveTo(box_x, box_y),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::DarkCyan),
            Print("╭"),
            Print("─".repeat((box_w.saturating_sub(2)) as usize)),
            Print("╮"),
            ResetColor
        )?;

        let visible_items = (box_h.saturating_sub(2)) as usize;
        for (i, (key, desc)) in items.iter().take(visible_items).enumerate() {
            let row_y = box_y + 1 + i as u16;
            execute!(
                w,
                MoveTo(box_x, row_y),
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
            let used = 2 + 2 + 3 + 18; // 25
            let pad = (box_w as usize).saturating_sub(used + 1);
            execute!(
                w,
                Print(" ".repeat(pad)),
                SetForegroundColor(Color::DarkCyan),
                Print("│"),
                ResetColor
            )?;
        }

        // Footer (ESC close)
        let footer_y = box_y + box_h - 1;
        let footer_text = " ESC close ";
        let dash_count = (box_w as usize).saturating_sub(footer_text.len() + 2);
        execute!(
            w,
            MoveTo(box_x, footer_y),
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
        )?;

        Ok(())
    }

    pub fn draw_copy_mode_badge<W: Write>(
        w: &mut W,
        rect: Rect,
        mode_label: &str,
        scroll_offset: usize,
        total_history: usize,
    ) -> io::Result<()> {
        let text = format!(" [{} | -{}/{} lines | y: yank | q: exit] ", mode_label, scroll_offset, total_history);
        let badge_x = (rect.x + rect.width).saturating_sub(text.len() as u16 + 1);
        if badge_x >= rect.x {
            execute!(
                w,
                MoveTo(badge_x, rect.y),
                SetBackgroundColor(Color::Rgb { r: 255, g: 140, b: 0 }),
                SetForegroundColor(Color::Black),
                Print(text),
                ResetColor
            )?;
        }
        Ok(())
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

        let cx = (pane_rect.x as usize + cursor_x).min((pane_rect.x + pane_rect.width).saturating_sub(1) as usize) as u16;
        let cy = (pane_rect.y as usize + cursor_y).min((pane_rect.y + pane_rect.height).saturating_sub(1) as usize) as u16;

        execute!(w, MoveTo(cx, cy), Show)?;
        w.flush()?;
        Ok(())
    }
}

fn render_row_cells<W: Write>(
    w: &mut W,
    row: usize,
    ctx: &RenderPaneContext<'_>,
    badge_info: Option<(usize, &[char])>,
) -> io::Result<()> {
    let abs_row = ctx.grid.abs_line(row, ctx.scroll_offset);
    execute!(w, MoveTo(ctx.rect.x, ctx.rect.y + row as u16))?;
    for col in 0..ctx.rect.width as usize {
        if row == 0 {
            if let Some((b_start, b_chars)) = badge_info {
                if col >= b_start && col < b_start + b_chars.len() {
                    let ch = b_chars[col - b_start];
                    execute!(
                        w,
                        SetBackgroundColor(Color::Rgb { r: 255, g: 140, b: 0 }),
                        SetForegroundColor(Color::Black),
                        Print(ch),
                        ResetColor
                    )?;
                    continue;
                }
            }
        }

        let cell = ctx.grid.get_cell_scrolled(col, row, ctx.scroll_offset).copied().unwrap_or_default();
        let is_selected = ctx.selection.map_or(false, |s| s.contains_abs(col, abs_row));
        if is_selected {
            execute!(
                w,
                SetBackgroundColor(Color::Rgb { r: 255, g: 140, b: 0 }),
                SetForegroundColor(Color::Black),
                Print(cell.c),
                ResetColor
            )?;
        } else {
            write_cell(w, cell)?;
        }
    }
    Ok(())
}

fn write_cell<W: Write>(w: &mut W, cell: Cell) -> io::Result<()> {
    match cell.style.fg {
        Some(fg) => execute!(w, SetForegroundColor(convert_color(fg)))?,
        None => execute!(w, SetForegroundColor(Color::Reset))?,
    }
    match cell.style.bg {
        Some(bg) => execute!(w, SetBackgroundColor(convert_color(bg)))?,
        None => execute!(w, SetBackgroundColor(Color::Reset))?,
    }

    execute!(w, Print(cell.c))?;
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
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt::grid::Grid;

    #[test]
    fn test_draw_pane_content() {
        let mut buf = Vec::new();
        let mut grid = Grid::new(10, 5);
        grid.put_char('A');
        let rect = Rect::new(0, 0, 10, 5);
        Compositor::draw_pane_content(
            &mut buf,
            RenderPaneContext {
                rect,
                grid: &grid,
                selection: None,
                scroll_offset: 0,
                badge: None,
            },
        )
        .unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains('A'));
    }

    #[test]
    fn test_draw_status_bar() {
        let mut buf = Vec::new();
        let panes = vec![("main".to_string(), Rect::new(0, 0, 80, 23), true)];
        Compositor::draw_status_bar(&mut buf, "sess", &[], &panes, false, 80, 23).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("[sess]"));
        assert!(out.contains("0:main*"));
    }

    #[test]
    fn test_draw_shortcut_hud() {
        let mut buf = Vec::new();
        Compositor::draw_shortcut_hud(&mut buf, 80, 24).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("╭"));
        assert!(out.contains("split vertical"));
        assert!(out.contains("split horizontal"));
        assert!(out.contains("ESC"));
    }

    #[test]
    fn test_draw_copy_mode_badge() {
        let mut buf = Vec::new();
        let rect = Rect::new(0, 0, 80, 24);
        Compositor::draw_copy_mode_badge(&mut buf, rect, "VISUAL LINE", 15, 100).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("VISUAL LINE"));
    }

    #[test]
    fn test_selection_range_line() {
        let range = SelectionRange::Line { start_abs_y: 2, end_abs_y: 4 };
        assert!(!range.contains_abs(5, 1));
        assert!(range.contains_abs(0, 2));
        assert!(range.contains_abs(50, 3));
        assert!(range.contains_abs(10, 4));
        assert!(!range.contains_abs(0, 5));
    }
}
