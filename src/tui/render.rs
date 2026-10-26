use std::io::{self, Write};
use crossterm::{
    cursor::MoveTo,
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use crate::layout::geometry::Rect;
use crate::vt::grid::{Cell, Grid};

pub struct Compositor;

impl Compositor {
    pub fn draw_borders<W: Write>(
        w: &mut W,
        rect: Rect,
        title: &str,
        is_focused: bool,
    ) -> io::Result<()> {
        if rect.width < 3 || rect.height < 3 {
            return Ok(());
        }

        let border_fg = if is_focused {
            Color::Cyan
        } else {
            Color::DarkGrey
        };

        execute!(w, SetForegroundColor(border_fg))?;

        // Top border with title
        execute!(w, MoveTo(rect.x, rect.y), Print("┌"))?;
        let title_str = format!(" {} ", title);
        let max_title_len = (rect.width.saturating_sub(4)) as usize;
        let displayed_title = if title_str.len() > max_title_len {
            &title_str[..max_title_len]
        } else {
            &title_str
        };

        execute!(
            w,
            SetForegroundColor(if is_focused { Color::White } else { Color::Grey }),
            Print(displayed_title),
            SetForegroundColor(border_fg)
        )?;

        let title_len = displayed_title.chars().count() as u16;
        let remaining_top = rect.width.saturating_sub(2 + title_len);
        execute!(w, Print("─".repeat(remaining_top as usize)), Print("┐"))?;

        // Side borders
        for y in (rect.y + 1)..(rect.y + rect.height.saturating_sub(1)) {
            execute!(w, MoveTo(rect.x, y), Print("│"))?;
            execute!(w, MoveTo(rect.x + rect.width.saturating_sub(1), y), Print("│"))?;
        }

        // Bottom border
        execute!(
            w,
            MoveTo(rect.x, rect.y + rect.height.saturating_sub(1)),
            Print("└"),
            Print("─".repeat((rect.width.saturating_sub(2)) as usize)),
            Print("┘")
        )?;

        execute!(w, ResetColor)?;
        Ok(())
    }

    pub fn draw_pane_content<W: Write>(
        w: &mut W,
        inner_rect: Rect,
        grid: &Grid,
    ) -> io::Result<()> {
        if inner_rect.width == 0 || inner_rect.height == 0 {
            return Ok(());
        }

        for row in 0..inner_rect.height as usize {
            if row >= grid.rows {
                break;
            }
            execute!(w, MoveTo(inner_rect.x, inner_rect.y + row as u16))?;
            for col in 0..inner_rect.width as usize {
                let cell = grid.get_cell(col, row).copied().unwrap_or_default();
                write_cell(w, cell)?;
            }
        }

        execute!(w, ResetColor)?;
        Ok(())
    }

    pub fn draw_status_bar<W: Write>(
        w: &mut W,
        session: &str,
        active_pane: &str,
        leader_active: bool,
        width: u16,
        y: u16,
    ) -> io::Result<()> {
        execute!(
            w,
            MoveTo(0, y),
            SetBackgroundColor(Color::DarkBlue),
            SetForegroundColor(Color::White)
        )?;

        let leader_text = if leader_active { " [CTRL-A ACTIVE] " } else { " " };
        let status = format!(
            " MPX: {} | Pane: {}{}│ [^A |: split-v] [^A -: split-h] [^A o: next] [^A d: detach]",
            session, active_pane, leader_text
        );

        let pad_len = (width as usize).saturating_sub(status.chars().count());
        execute!(w, Print(status), Print(" ".repeat(pad_len)), ResetColor)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt::grid::Grid;

    #[test]
    fn test_compositor_draw_borders() {
        let mut buf = Vec::new();
        let rect = Rect::new(0, 0, 20, 10);
        Compositor::draw_borders(&mut buf, rect, "test-pane", true).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("test-pane"));
        assert!(out.contains("┌"));
        assert!(out.contains("┐"));
        assert!(out.contains("└"));
        assert!(out.contains("┘"));
    }

    #[test]
    fn test_compositor_draw_status_bar() {
        let mut buf = Vec::new();
        Compositor::draw_status_bar(&mut buf, "sess1", "pane1", true, 80, 23).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains("MPX: sess1"));
        assert!(out.contains("Pane: pane1"));
        assert!(out.contains("CTRL-A ACTIVE"));
    }

    #[test]
    fn test_compositor_draw_pane_content() {
        let mut buf = Vec::new();
        let mut grid = Grid::new(10, 5);
        grid.put_char('Z');
        let inner = Rect::new(1, 1, 10, 5);
        Compositor::draw_pane_content(&mut buf, inner, &grid).unwrap();
        let out = String::from_utf8_lossy(&buf);
        assert!(out.contains('Z'));
    }
}
