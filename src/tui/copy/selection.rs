//! The selection: its shape and anchor, and the text it covers.
use crate::tui::render::SelectionRange;
use crate::vt::grid::Grid;

use super::{CopyState, SelectionKind};

impl CopyState {
    pub fn selection_range(&self, grid: &Grid) -> Option<SelectionRange> {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::None => None,
            SelectionKind::Line => Some(SelectionRange::Line {
                start_abs_y: self.anchor_abs_y,
                end_abs_y: cursor_abs_y,
            }),
            SelectionKind::Rectangle => Some(SelectionRange::Rectangle {
                anchor_x: self.anchor_x,
                anchor_abs_y: self.anchor_abs_y,
                cursor_x: self.cursor_x,
                cursor_abs_y,
            }),
            SelectionKind::Character => Some(SelectionRange::Character {
                anchor_x: self.anchor_x,
                anchor_abs_y: self.anchor_abs_y,
                cursor_x: self.cursor_x,
                cursor_abs_y,
            }),
        }
    }

    /// True when the cursor sits exactly on the anchor, i.e. nothing is selected.
    pub fn is_empty_selection(&self, grid: &Grid) -> bool {
        self.anchor_x == self.cursor_x && self.anchor_abs_y == self.cursor_abs_y(grid)
    }

    pub fn yank_text(&self, grid: &Grid) -> String {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::Line => yank_lines(grid, self.anchor_abs_y, cursor_abs_y),
            SelectionKind::Rectangle => yank_rect(
                grid,
                self.anchor_x,
                self.cursor_x,
                self.anchor_abs_y,
                cursor_abs_y,
            ),
            SelectionKind::Character => yank_chars(grid, self, cursor_abs_y),
            SelectionKind::None => line_text(grid, cursor_abs_y, 0),
        }
    }

    pub(super) fn toggle_line(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Line {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Line;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = 0;
    }

    pub(super) fn toggle_char(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Character {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Character;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = self.cursor_x;
    }

    /// `rectangle-toggle`: turns a rectangle selection into a character one and back; a character
    /// selection keeps its anchor, and with no selection a rectangle one starts at the cursor.
    pub(super) fn toggle_rectangle(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Rectangle {
            self.mode = SelectionKind::Character;
            return;
        }
        if self.mode != SelectionKind::Character {
            self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
            self.anchor_x = self.cursor_x;
        }
        self.mode = SelectionKind::Rectangle;
    }
}

fn row_chars(grid: &Grid, abs_y: usize) -> Option<Vec<char>> {
    grid.line_cells(abs_y)
        .map(|cells| cells.iter().map(|c| c.c).collect())
}

/// The text of one history line from column `from`, trailing blanks dropped.
pub(super) fn line_text(grid: &Grid, abs_y: usize, from: usize) -> String {
    row_chars(grid, abs_y).map_or_else(String::new, |cells| {
        cells
            .into_iter()
            .skip(from)
            .collect::<String>()
            .trim_end()
            .to_string()
    })
}

fn yank_lines(grid: &Grid, anchor_abs_y: usize, cursor_abs_y: usize) -> String {
    yank_rows(grid, anchor_abs_y, cursor_abs_y, |cells| {
        cells.iter().collect()
    })
}

/// The rows between two absolute lines, each cut by `cut` and stripped of trailing blanks.
fn yank_rows(
    grid: &Grid,
    anchor_abs_y: usize,
    cursor_abs_y: usize,
    cut: impl Fn(&[char]) -> String,
) -> String {
    let (top, bot) = (
        anchor_abs_y.min(cursor_abs_y),
        anchor_abs_y.max(cursor_abs_y),
    );
    (top..=bot)
        .filter_map(|y| row_chars(grid, y))
        .map(|cells| cut(&cells).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn yank_chars(grid: &Grid, cs: &CopyState, cursor_abs_y: usize) -> String {
    let anchor = (cs.anchor_x, cs.anchor_abs_y);
    let cursor = (cs.cursor_x, cursor_abs_y);
    let top = anchor.1.min(cursor.1);
    let bot = anchor.1.max(cursor.1);
    let lines: Vec<Option<Vec<char>>> = (top..=bot).map(|y| row_chars(grid, y)).collect();
    char_selection_text(&lines, anchor, cursor)
}

/// Text of the rectangle between two corners (`rectangle-toggle`): the columns between the
/// anchor's and the cursor's, on every row between their rows. Trailing blanks of each row go.
fn yank_rect(
    grid: &Grid,
    anchor_x: usize,
    cursor_x: usize,
    anchor_abs_y: usize,
    cursor_abs_y: usize,
) -> String {
    let (from, to) = (anchor_x.min(cursor_x), anchor_x.max(cursor_x));
    yank_rows(grid, anchor_abs_y, cursor_abs_y, |cells| {
        row_text(cells, from, to).unwrap_or_default()
    })
}

/// Text covered by a character selection between `anchor` and `cursor` (order-independent).
pub fn char_selection_text(
    lines: &[Option<Vec<char>>],
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> String {
    let (first, last) = ordered_span(anchor, cursor);
    let (from_col, from_row) = first;
    let (to_col, to_row) = last;
    let row_count = to_row.saturating_sub(from_row) + 1;
    let mut parts: Vec<String> = Vec::new();
    for (i, row) in lines.iter().take(row_count).enumerate() {
        let Some(cells) = row else { continue };
        let text = if row_count == 1 {
            row_text(cells, from_col, to_col)
        } else if i == 0 {
            row_text(cells, from_col, cells.len().saturating_sub(1))
        } else if i + 1 == row_count {
            row_text(cells, 0, to_col)
        } else {
            row_text(cells, 0, cells.len().saturating_sub(1))
        };
        if let Some(t) = text {
            parts.push(t.trim_end().to_string());
        }
    }
    parts.join("\n")
}

fn ordered_span(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Cells `from..=to` of one row as text; `None` when the span is empty.
fn row_text(cells: &[char], from: usize, to: usize) -> Option<String> {
    if from > to || from >= cells.len() {
        return None;
    }
    let end = (to + 1).min(cells.len());
    Some(cells[from..end].iter().collect())
}
