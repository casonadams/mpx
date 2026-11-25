//! A screen as the escape sequences that rebuild it: fed to a fresh screen of the same size they
//! give the same cells, cursor, cursor shape, margins and modes the grid has, whatever output
//! produced them.
use std::fmt::Write;

use super::capture::push_sgr;
use super::{Cell, CellStyle, CursorShape, Grid, LinkId};

/// The sequences that paint `grid` onto a fresh screen of its own size.
pub fn keyframe(grid: &Grid) -> String {
    let mut out = String::new();
    if grid.alternate_screen() {
        out.push_str("\x1b[0m\x1b[?69l\x1b[r\x1b[4l\x1b[?6l\x1b[?7h");
        let hidden = grid.hidden_screen_lines().collect::<Vec<_>>();
        if hidden
            .iter()
            .any(|(row, _)| row.iter().any(|c| *c != Cell::default()))
        {
            paint_rows(&mut out, hidden.into_iter());
        }
        if let Some(saved) = grid.saved_cursor {
            let _ = write!(out, "\x1b[{};{}H", saved.y + 1, saved.x + 1);
        }
        out.push_str("\x1b[?1049h");
    }
    out.push_str("\x1b[0m\x1b[?69l\x1b[r\x1b[4l\x1b[?6l\x1b[?7h");
    let rows = grid.screen_lines().enumerate().map(|(y, row)| {
        (
            row,
            y + 1 < grid.rows && grid.line_wrapped(grid.history_len() + y),
        )
    });
    paint_rows(&mut out, rows);

    if grid.scroll_top != 0 || grid.scroll_bottom + 1 != grid.rows {
        let _ = write!(
            out,
            "\x1b[{};{}r",
            grid.scroll_top + 1,
            grid.scroll_bottom + 1
        );
    }
    if grid.lr_margins {
        let _ = write!(
            out,
            "\x1b[?69h\x1b[{};{}s",
            grid.scroll_left + 1,
            grid.scroll_right + 1
        );
    }
    if let Some(saved) = grid.saved_cursor {
        push_visibility(&mut out, saved.visible);
        let _ = write!(out, "\x1b[{};{}H\x1b7", saved.y + 1, saved.x + 1);
    }
    let (origin_x, origin_y) = grid.addressed_position(0, 0);
    if grid.origin_mode {
        out.push_str("\x1b[?6h");
    }
    let cursor = grid.cursor;
    if cursor.x >= grid.cols {
        // Waiting to wrap: print the last cell again to leave the cursor past it.
        let last = grid
            .get_cell(grid.cols - 1, cursor.y)
            .copied()
            .unwrap_or_default();
        let _ = write!(
            out,
            "\x1b[{};{}H",
            (cursor.y + 1).saturating_sub(origin_y),
            grid.cols - origin_x
        );
        push_sgr(&mut out, last.style);
        out.push(last.c);
    } else {
        let _ = write!(
            out,
            "\x1b[{};{}H",
            (cursor.y + 1).saturating_sub(origin_y),
            (cursor.x + 1).saturating_sub(origin_x)
        );
    }
    push_sgr(&mut out, grid.current_style);
    push_visibility(&mut out, cursor.visible);
    if grid.cursor_shape != CursorShape::Default {
        out.push_str(&grid.cursor_shape.sequence());
    }
    if !grid.autowrap {
        out.push_str("\x1b[?7l");
    }
    if grid.insert_mode {
        out.push_str("\x1b[4h");
    }
    out
}

/// Paints `rows` from the top of the screen, each with whether it wraps into the next, and
/// leaves the pen and link reset.
fn paint_rows<'a>(out: &mut String, rows: impl Iterator<Item = (&'a [Cell], bool)>) {
    let mut style = CellStyle::default();
    let mut link: Option<LinkId> = None;
    let mut continued = false;
    for (y, (row, wraps)) in rows.enumerate() {
        if !continued {
            let _ = write!(out, "\x1b[{};1H", y + 1);
        }
        for cell in row {
            if cell.style != style {
                push_sgr(out, cell.style);
                style = cell.style;
            }
            if cell.style.link != link {
                push_link(out, cell.style.link);
                link = cell.style.link;
            }
            out.push(cell.c);
        }
        continued = wraps;
    }
    push_link(out, None);
    out.push_str("\x1b[0m");
}

fn push_link(out: &mut String, link: Option<LinkId>) {
    match link {
        Some(id) => id.with_body(|body| {
            let _ = write!(out, "\x1b]8;{body}\x1b\\");
        }),
        None => out.push_str("\x1b]8;;\x1b\\"),
    }
}

fn push_visibility(out: &mut String, visible: bool) {
    out.push_str(if visible { "\x1b[?25h" } else { "\x1b[?25l" });
}
