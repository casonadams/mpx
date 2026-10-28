//! Copy (vi-style visual) mode: cursor, scroll, selection and yank.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::render::SelectionRange;
use crate::vt::grid::Grid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    None,
    Character,
    Line,
}

#[derive(Debug, Clone)]
pub struct CopyState {
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub scroll_offset: usize,
    pub anchor_x: usize,
    pub anchor_abs_y: usize,
    pub mode: SelectionKind,
}

/// What the key handler asks the app to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    Stay,
    Exit,
    Yank(String),
}

/// Pane size in cells, used to clamp cursor motion.
pub type PaneSize = (u16, u16);

/// Rows moved by one mouse-wheel notch.
pub const WHEEL_STEP: usize = 3;
const PAGE: usize = 15;
const WORD: usize = 4;

impl CopyState {
    /// Copy mode entered at the pane's cursor.
    pub fn enter(grid: Option<&Grid>) -> Self {
        let (x, y) = grid.map_or((0, 0), |g| (g.cursor.x, g.cursor.y));
        let abs_y = grid.map_or(0, |g| g.abs_line(y, 0));
        Self {
            cursor_x: x,
            cursor_y: y,
            scroll_offset: 0,
            anchor_x: x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::None,
        }
    }

    /// Copy mode entered by a mouse press at a local cell position.
    pub fn at_point(local_x: usize, local_y: usize, grid: Option<&Grid>) -> Self {
        let abs_y = grid.map_or(local_y, |g| g.abs_line(local_y, 0));
        Self {
            cursor_x: local_x,
            cursor_y: local_y,
            scroll_offset: 0,
            anchor_x: local_x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::Character,
        }
    }

    /// Copy mode entered by the wheel from the pane's cursor. With scrollback the view scrolls
    /// into history; without it the cursor moves up.
    pub fn from_wheel(grid: Option<&Grid>, hist_len: usize) -> Self {
        let (x, y, abs_y) = grid.map_or((0, 0, 0), |g| {
            (g.cursor.x, g.cursor.y, g.abs_line(g.cursor.y, 0))
        });
        Self {
            cursor_x: x,
            cursor_y: if hist_len == 0 {
                y.saturating_sub(WHEEL_STEP)
            } else {
                y
            },
            scroll_offset: WHEEL_STEP.min(hist_len),
            anchor_x: x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::None,
        }
    }

    /// One wheel notch up: scroll into history while there is more of it, then move the cursor.
    pub fn wheel_up(&mut self, hist_len: usize) {
        if hist_len > 0 && self.scroll_offset < hist_len {
            self.scroll_offset = (self.scroll_offset + WHEEL_STEP).min(hist_len);
        } else {
            self.cursor_y = self.cursor_y.saturating_sub(WHEEL_STEP);
        }
    }

    /// One wheel notch down: scroll back toward the live screen, then move the cursor down.
    pub fn wheel_down(&mut self, pane_h: usize) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(WHEEL_STEP);
        } else {
            self.cursor_y = (self.cursor_y + WHEEL_STEP).min(pane_h.saturating_sub(1));
        }
    }

    pub fn selection_range(&self, grid: &Grid) -> Option<SelectionRange> {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::None => None,
            SelectionKind::Line => Some(SelectionRange::Line {
                start_abs_y: self.anchor_abs_y,
                end_abs_y: cursor_abs_y,
            }),
            SelectionKind::Character => Some(SelectionRange::Character {
                anchor_x: self.anchor_x,
                anchor_abs_y: self.anchor_abs_y,
                cursor_x: self.cursor_x,
                cursor_abs_y,
            }),
        }
    }

    pub fn cursor_abs_y(&self, grid: &Grid) -> usize {
        grid.abs_line(self.cursor_y, self.scroll_offset)
    }

    /// True when the cursor sits exactly on the anchor, i.e. nothing is selected.
    pub fn is_empty_selection(&self, grid: &Grid) -> bool {
        self.anchor_x == self.cursor_x && self.anchor_abs_y == self.cursor_abs_y(grid)
    }

    pub fn yank_text(&self, grid: &Grid) -> String {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::Line => yank_lines(grid, self.anchor_abs_y, cursor_abs_y),
            SelectionKind::Character => yank_chars(grid, self, cursor_abs_y),
            SelectionKind::None => grid
                .get_line_cells(cursor_abs_y)
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| c.c)
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .unwrap_or_default(),
        }
    }

    fn toggle_line(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Line {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Line;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = 0;
    }

    fn toggle_char(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Character {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Character;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = self.cursor_x;
    }
}

fn yank_lines(grid: &Grid, anchor_abs_y: usize, cursor_abs_y: usize) -> String {
    let (min_y, max_y) = (
        anchor_abs_y.min(cursor_abs_y),
        anchor_abs_y.max(cursor_abs_y),
    );
    (min_y..=max_y)
        .filter_map(|y| grid.get_line_cells(y))
        .map(|cells| {
            cells
                .iter()
                .map(|c| c.c)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn yank_chars(grid: &Grid, cs: &CopyState, cursor_abs_y: usize) -> String {
    let anchor = (cs.anchor_x, cs.anchor_abs_y);
    let cursor = (cs.cursor_x, cursor_abs_y);
    let top = anchor.1.min(cursor.1);
    let bottom = anchor.1.max(cursor.1);
    let lines: Vec<Option<Vec<char>>> = (top..=bottom)
        .map(|y| {
            grid.get_line_cells(y)
                .map(|cells| cells.iter().map(|c| c.c).collect())
        })
        .collect();
    char_selection_text(&lines, anchor, cursor)
}

/// Text of a character-wise selection. `lines[i]` is the row at absolute line `top + i`, where
/// `top` is the upper of the two endpoints; `None` rows are skipped. Endpoints are `(x, abs_y)`.
/// Interior rows are right-trimmed and each row break is a `\n`.
pub fn char_selection_text(
    lines: &[Option<Vec<char>>],
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> String {
    let ((sx, sy), (ex, ey)) = ordered_span(anchor, cursor);
    let mut out = String::new();
    for y in sy..=ey {
        let Some(Some(cells)) = lines.get(y - sy) else {
            continue;
        };
        let from = if y == sy { sx } else { 0 };
        let to = if y == ey {
            ex
        } else {
            cells.len().saturating_sub(1)
        };
        let Some(row) = row_text(cells, from, to) else {
            continue;
        };
        if y < ey {
            out.push_str(row.trim_end());
            out.push('\n');
        } else {
            out.push_str(&row);
        }
    }
    out
}

/// The earlier of two `(x, abs_y)` endpoints first.
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

/// Applies one key in copy mode. `grid` is the active pane's screen; `pane` its size in cells.
pub fn apply_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    let hist_len = grid.map_or(0, |g| g.history_len());
    if move_cursor(cs, key, hist_len, pane) {
        return CopyOutcome::Stay;
    }
    select_or_exit(cs, key, grid)
}

fn move_cursor(cs: &mut CopyState, key: &KeyEvent, hist_len: usize, pane: PaneSize) -> bool {
    let (pane_w, pane_h) = (pane.0 as usize, pane.1 as usize);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let last_col = pane_w.saturating_sub(1);
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => cs.cursor_x = cs.cursor_x.saturating_sub(1),
        KeyCode::Char('l') | KeyCode::Right => cs.cursor_x = (cs.cursor_x + 1).min(last_col),
        KeyCode::Char('k') | KeyCode::Up => {
            if cs.cursor_y > 0 {
                cs.cursor_y -= 1;
            } else if cs.scroll_offset < hist_len {
                cs.scroll_offset += 1;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if cs.cursor_y + 1 < pane_h {
                cs.cursor_y += 1;
            } else if cs.scroll_offset > 0 {
                cs.scroll_offset -= 1;
            }
        }
        KeyCode::Char('0') | KeyCode::Char('^') => cs.cursor_x = 0,
        KeyCode::Char('$') => cs.cursor_x = last_col,
        KeyCode::Char('w') => cs.cursor_x = (cs.cursor_x + WORD).min(last_col),
        KeyCode::Char('b') => cs.cursor_x = cs.cursor_x.saturating_sub(WORD),
        KeyCode::Char('u') if ctrl => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        KeyCode::Char('d') if ctrl => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        KeyCode::PageUp => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        KeyCode::PageDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        KeyCode::Char('g') => {
            cs.scroll_offset = hist_len;
            cs.cursor_y = 0;
            cs.cursor_x = 0;
        }
        KeyCode::Char('G') => {
            cs.scroll_offset = 0;
            cs.cursor_y = pane_h.saturating_sub(1);
        }
        _ => return false,
    }
    true
}

fn select_or_exit(cs: &mut CopyState, key: &KeyEvent, grid: Option<&Grid>) -> CopyOutcome {
    match key.code {
        KeyCode::Char('V') => cs.toggle_line(grid),
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::SHIFT) => cs.toggle_line(grid),
        KeyCode::Char('v') => cs.toggle_char(grid),
        KeyCode::Char('y') | KeyCode::Enter => {
            return match grid {
                Some(g) => CopyOutcome::Yank(cs.yank_text(g)),
                None => CopyOutcome::Exit,
            };
        }
        KeyCode::Esc => {
            if cs.mode == SelectionKind::None {
                return CopyOutcome::Exit;
            }
            cs.mode = SelectionKind::None;
        }
        KeyCode::Char('q') => return CopyOutcome::Exit,
        _ => {}
    }
    CopyOutcome::Stay
}
