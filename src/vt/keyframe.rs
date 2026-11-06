//! A screen as the escape sequences that rebuild it: fed to a fresh screen of the same size they
//! give the same cells, cursor, margins and modes the grid has, whatever output produced them.
use std::fmt::Write;

use super::capture::push_sgr;
use super::{CellStyle, Grid, LinkId};

/// The sequences that paint `grid` onto a fresh screen of its own size.
pub fn keyframe(grid: &Grid) -> String {
    let mut out = String::new();
    if grid.alternate_screen() {
        out.push_str("\x1b[?1049h");
    }
    out.push_str("\x1b[0m\x1b[?69l\x1b[r");
    let mut style = CellStyle::default();
    let mut link: Option<LinkId> = None;
    let mut continued = false;
    for (y, row) in grid.screen_lines().enumerate() {
        if !continued {
            let _ = write!(out, "\x1b[{};1H", y + 1);
        }
        for cell in row {
            if cell.style != style {
                push_sgr(&mut out, cell.style);
                style = cell.style;
            }
            if cell.style.link != link {
                push_link(&mut out, cell.style.link);
                link = cell.style.link;
            }
            out.push(cell.c);
        }
        let below = y + 1 < grid.rows;
        continued = below && grid.line_wrapped(grid.history_len() + y);
    }
    push_link(&mut out, None);
    out.push_str("\x1b[0m");

    if grid.scroll_top != 0 || grid.scroll_bottom + 1 != grid.rows {
        let _ = write!(out, "\x1b[{};{}r", grid.scroll_top + 1, grid.scroll_bottom + 1);
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
    let cursor = grid.cursor;
    if cursor.x >= grid.cols {
        // Waiting to wrap: print the last cell again to leave the cursor past it.
        let last = grid.get_cell(grid.cols - 1, cursor.y).copied().unwrap_or_default();
        let _ = write!(out, "\x1b[{};{}H", cursor.y + 1, grid.cols);
        push_sgr(&mut out, last.style);
        out.push(last.c);
    } else {
        let _ = write!(out, "\x1b[{};{}H", cursor.y + 1, cursor.x + 1);
    }
    push_sgr(&mut out, grid.current_style);
    push_visibility(&mut out, cursor.visible);
    out
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
