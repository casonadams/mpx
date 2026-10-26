use std::io::{self, Write};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use crate::layout::{geometry::Rect, Divider, WindowTab};
use crate::vt::grid::{Cell, Grid};

pub struct Compositor;

impl Compositor {
    /// Renders terminal pane content directly into its bounding box
    pub fn draw_pane_content<W: Write>(
        w: &mut W,
        rect: Rect,
        grid: &Grid,
    ) -> io::Result<()> {
        if rect.width == 0 || rect.height == 0 {
            return Ok(());
        }

        for row in 0..rect.height as usize {
            execute!(w, MoveTo(rect.x, rect.y + row as u16))?;
            for col in 0..rect.width as usize {
                let cell = grid.get_cell(col, row).copied().unwrap_or_default();
                write_cell(w, cell)?;
            }
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
    /// Floating modern horizontal HUD chip rendered in bottom-right corner when leader is active
    pub fn draw_shortcut_hud<W: Write>(w: &mut W, screen_w: u16, screen_h: u16) -> io::Result<()> {
        let box_w: u16 = 60.min(screen_w.saturating_sub(2));
        let box_h: u16 = 4;
        if box_w < 35 || screen_h < box_h + 1 {
            return Ok(());
        }

        let box_x = screen_w.saturating_sub(box_w + 1);
        let box_y = screen_h.saturating_sub(box_h + 1);

        // Header border
        execute!(
            w,
            MoveTo(box_x, box_y),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::Cyan),
            Print("┌"),
            Print("─".repeat((box_w.saturating_sub(2)) as usize)),
            Print("┐"),
            ResetColor
        )?;

        let rows = [
            [
                ("c", "win"),
                ("%", "split-v"),
                ("\"", "split-h"),
                ("o", "pane"),
                ("z", "zoom"),
            ],
            [
                ("n", "next"),
                ("p", "prev"),
                ("[", "copy"),
                ("x", "kill"),
                ("d", "detach"),
            ],
        ];

        for (i, row) in rows.iter().enumerate() {
            let row_y = box_y + 1 + i as u16;
            execute!(
                w,
                MoveTo(box_x, row_y),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Cyan),
                Print("│")
            )?;

            for (j, (k, desc)) in row.iter().enumerate() {
                execute!(
                    w,
                    SetBackgroundColor(Color::Black),
                    SetForegroundColor(Color::Yellow),
                    Print(format!(" {} ", k)),
                    SetForegroundColor(Color::White),
                    Print(format!("{:<6}", desc)),
                )?;
                if j < row.len() - 1 {
                    execute!(
                        w,
                        SetForegroundColor(Color::DarkGrey),
                        Print("│"),
                    )?;
                }
            }

            let used = 1 + 5 * 9 + 4 * 1;
            let pad = (box_w as usize).saturating_sub(used + 1);
            execute!(
                w,
                Print(" ".repeat(pad)),
                SetForegroundColor(Color::Cyan),
                Print("│"),
                ResetColor
            )?;
        }

        // Bottom border
        execute!(
            w,
            MoveTo(box_x, box_y + box_h - 1),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::Cyan),
            Print("└"),
            Print("─".repeat((box_w.saturating_sub(2)) as usize)),
            Print("┘"),
            ResetColor
        )?;

        Ok(())
    }

    pub fn draw_copy_mode_badge<W: Write>(
        w: &mut W,
        rect: Rect,
        scroll_offset: usize,
    ) -> io::Result<()> {
        let text = format!(" [COPY MODE: -{} lines | v: select | y: copy | q: exit] ", scroll_offset);
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

fn write_cell<W: Write>(w: &mut W, cell: Cell) -> io::Result<()> {
    if let Some(fg) = cell.style.fg {
        execute!(w, SetForegroundColor(convert_color(fg)))?;
    }
    if let Some(bg) = cell.style.bg {
        execute!(w, SetBackgroundColor(convert_color(bg)))?;
    }

    execute!(w, Print(cell.c))?;
    Ok(())
}

fn convert_color(c: crate::vt::grid::Color) -> Color {
    match c {
        crate::vt::grid::Color::Black => Color::Black,
        crate::vt::grid::Color::Red => Color::DarkRed,
        crate::vt::grid::Color::Green => Color::DarkGreen,
        crate::vt::grid::Color::Yellow => Color::DarkYellow,
        crate::vt::grid::Color::Blue => Color::DarkBlue,
        crate::vt::grid::Color::Magenta => Color::DarkMagenta,
        crate::vt::grid::Color::Cyan => Color::DarkCyan,
        crate::vt::grid::Color::White => Color::Grey,
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
    // Standard HH:MM without heavy external chrono dependency
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    format!("{:02}:{:02}", hours, mins)
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
        Compositor::draw_pane_content(&mut buf, rect, &grid).unwrap();
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
        assert!(out.contains("┌"));
        assert!(out.contains("split-v"));
        assert!(out.contains("split-h"));
        assert!(out.contains("detach"));
    }

    #[test]
    fn test_draw_copy_mode_badge() {
        let mut buf = Vec::new();
        let rect = Rect::new(0, 0, 80, 24);
        Compositor::draw_copy_mode_badge(&mut buf, rect, 15).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("COPY MODE: -15 lines"));
    }
}
