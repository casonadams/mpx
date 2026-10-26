use std::io::{self, Write};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use crate::layout::geometry::Rect;
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

    /// Renders thin tmux-style single-line dividers between panes
    pub fn draw_dividers<W: Write>(
        w: &mut W,
        rect: Rect,
        screen_w: u16,
        screen_h: u16,
        is_focused: bool,
    ) -> io::Result<()> {
        let border_fg = if is_focused {
            Color::Green
        } else {
            Color::DarkGrey
        };

        execute!(w, SetForegroundColor(border_fg))?;

        // Draw vertical right divider if not at the right screen edge
        let right_x = rect.x + rect.width;
        if right_x < screen_w {
            for y in rect.y..(rect.y + rect.height) {
                execute!(w, MoveTo(right_x, y), Print("│"))?;
            }
        }

        // Draw horizontal bottom divider if not at the bottom statusline edge
        let bottom_y = rect.y + rect.height;
        if bottom_y < screen_h {
            execute!(w, MoveTo(rect.x, bottom_y))?;
            for _ in 0..rect.width {
                execute!(w, Print("─"))?;
            }
        }

        execute!(w, ResetColor)?;
        Ok(())
    }

    /// Renders a clean tmux-style status line at the bottom
    pub fn draw_status_bar<W: Write>(
        w: &mut W,
        session: &str,
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
        for (i, (name, _, is_focused)) in panes.iter().enumerate() {
            let active_mark = if *is_focused { "*" } else { "-" };
            left.push_str(&format!("{}:{}{} ", i, name, active_mark));
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
    /// Floating which-key popup rendered when leader prefix is triggered
    pub fn draw_which_key<W: Write>(w: &mut W, screen_w: u16, screen_h: u16) -> io::Result<()> {
        let box_w: u16 = 58.min(screen_w.saturating_sub(4));
        let box_h: u16 = 6;
        if box_w < 30 || screen_h < box_h + 2 {
            return Ok(());
        }

        let box_x = (screen_w.saturating_sub(box_w)) / 2;
        let box_y = screen_h.saturating_sub(box_h + 2);

        // Header - identical width to footer
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

        let items = [
            (" % / | / v ", "split-v", " \" / - / s ", "split-h"),
            (" o / Tab   ", "next-pane", " p         ", "prev-pane"),
            (" z         ", "zoom-pane", " x         ", "kill-pane"),
            (" c         ", "new-pane", " d         ", "detach"),
        ];

        for (i, (k1, d1, k2, d2)) in items.iter().enumerate() {
            let row_y = box_y + 1 + i as u16;
            execute!(
                w,
                MoveTo(box_x, row_y),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Cyan),
                Print("│"),
                SetForegroundColor(Color::Yellow),
                Print(*k1),
                SetForegroundColor(Color::White),
                Print(format!("{:<11}", d1)),
                SetForegroundColor(Color::DarkGrey),
                Print(" │ "),
                SetForegroundColor(Color::Yellow),
                Print(*k2),
                SetForegroundColor(Color::White),
                Print(format!("{:<11}", d2)),
            )?;
            let used = 1 + 11 + 11 + 3 + 11 + 11;
            let pad = (box_w.saturating_sub(used as u16 + 1)) as usize;
            execute!(
                w,
                Print(" ".repeat(pad)),
                SetForegroundColor(Color::Cyan),
                Print("│"),
                ResetColor
            )?;
        }

        // Footer
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
        Compositor::draw_status_bar(&mut buf, "sess", &panes, false, 80, 23).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("[sess]"));
        assert!(out.contains("0:main*"));
    }

    #[test]
    fn test_draw_which_key() {
        let mut buf = Vec::new();
        Compositor::draw_which_key(&mut buf, 80, 24).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("┌"));
        assert!(out.contains("split-v"));
        assert!(out.contains("split-h"));
        assert!(out.contains("detach"));
    }
}
