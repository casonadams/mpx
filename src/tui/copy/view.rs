//! Where copy mode stands: how it is entered, the cursor and scroll position, and the moves that
//! keep a position on screen.
use crate::vt::grid::Grid;

use super::{CopyState, SelectionKind, WHEEL_STEP};

impl CopyState {
    fn fresh(x: usize, y: usize, abs_y: usize) -> Self {
        Self {
            cursor_x: x,
            cursor_y: y,
            anchor_x: x,
            anchor_abs_y: abs_y,
            wrap_search: true,
            ..Self::default()
        }
    }

    /// Copy mode entered at the pane's cursor.
    pub fn enter(grid: Option<&Grid>) -> Self {
        let (x, y) = grid.map_or((0, 0), |g| (g.cursor.x, g.cursor.y));
        let abs_y = grid.map_or(0, |g| g.abs_line(y, 0));
        Self::fresh(x, y, abs_y)
    }

    /// Copy mode entered by a mouse press at a local cell position.
    pub fn at_point(local_x: usize, local_y: usize, grid: Option<&Grid>) -> Self {
        let abs_y = grid.map_or(local_y, |g| g.abs_line(local_y, 0));
        Self {
            mode: SelectionKind::Character,
            ..Self::fresh(local_x, local_y, abs_y)
        }
    }

    /// A click inside a pane already in copy mode: the cursor and selection anchor move to the
    /// cell, any selection is dropped and the scroll position stays where it was.
    pub fn click(&mut self, local_x: usize, local_y: usize, grid: Option<&Grid>) {
        self.cursor_x = local_x;
        self.cursor_y = local_y;
        self.anchor_x = local_x;
        self.anchor_abs_y = grid.map_or(local_y, |g| g.abs_line(local_y, self.scroll_offset));
        self.mode = SelectionKind::None;
    }

    /// Drag to a cell: starts a character selection from the click anchor if none is active.
    pub fn drag_to(&mut self, local_x: usize, local_y: usize) {
        if self.mode == SelectionKind::None {
            self.mode = SelectionKind::Character;
        }
        self.cursor_x = local_x;
        self.cursor_y = local_y;
    }

    /// Copy mode entered by the wheel from the pane's cursor. With scrollback the view scrolls
    /// into history; without it the cursor moves up.
    pub fn from_wheel(grid: Option<&Grid>, hist_len: usize) -> Self {
        let (x, y, abs_y) = grid.map_or((0, 0, 0), |g| {
            (g.cursor.x, g.cursor.y, g.abs_line(g.cursor.y, 0))
        });
        let cursor_y = if hist_len == 0 {
            y.saturating_sub(WHEEL_STEP)
        } else {
            y
        };
        Self {
            cursor_y,
            scroll_offset: WHEEL_STEP.min(hist_len),
            wheel_exit: true,
            ..Self::fresh(x, y, abs_y)
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

    /// One wheel notch down, back toward the live screen. Returns true once it shows the live
    /// screen: scrolling the wheel back down to it leaves copy mode, as in tmux.
    pub fn wheel_down(&mut self) -> bool {
        self.scroll_offset = self.scroll_offset.saturating_sub(WHEEL_STEP);
        self.scroll_offset == 0
    }

    /// Keeps the copy position through a resize of its pane (`grid` is the rebuilt screen, `old_hist`
    /// the history it had): the view stays on the same lines, moved by the change in history (rows
    /// pulled back from it, or pushed into it), and the cursor stays on the screen. A width change
    /// reflows the lines, so the mark, a row number, no longer names its line and is dropped.
    pub fn resized(&mut self, old_hist: usize, grid: &Grid, reflowed: bool) {
        let hist = grid.history_len();
        self.scroll_offset = (self.scroll_offset + hist)
            .saturating_sub(old_hist)
            .min(hist);
        let last_col = grid.cols.saturating_sub(1);
        self.cursor_x = self.cursor_x.min(last_col);
        self.anchor_x = self.anchor_x.min(last_col);
        self.cursor_y = self.cursor_y.min(grid.rows.saturating_sub(1));
        if reflowed {
            self.mark = None;
        }
    }

    pub fn scroll_to_match(
        &mut self,
        match_x: usize,
        match_abs_y: usize,
        total_hist: usize,
        rows: usize,
    ) {
        let rows = rows.max(1);
        let top = total_hist.saturating_sub(self.scroll_offset);
        let bot = (total_hist + rows.saturating_sub(1)).saturating_sub(self.scroll_offset);
        if match_abs_y >= top && match_abs_y <= bot {
            self.cursor_y = match_abs_y - top;
        } else {
            self.scroll_to_abs_y(match_abs_y, total_hist, rows);
        }
        self.cursor_x = match_x;
    }

    pub fn scroll_to_abs_y(&mut self, abs_y: usize, total_hist: usize, rows: usize) {
        let rows = rows.max(1);
        let target_screen_y = (rows / 2).min(rows.saturating_sub(1));
        if abs_y >= total_hist {
            self.scroll_offset = 0;
            self.cursor_y = (abs_y - total_hist).min(rows.saturating_sub(1));
        } else {
            let ideal_scroll = (total_hist + target_screen_y).saturating_sub(abs_y);
            self.scroll_offset = ideal_scroll.min(total_hist);
            let actual_top = total_hist.saturating_sub(self.scroll_offset);
            self.cursor_y = abs_y.saturating_sub(actual_top).min(rows.saturating_sub(1));
        }
    }

    pub fn cursor_abs_y(&self, grid: &Grid) -> usize {
        grid.abs_line(self.cursor_y, self.scroll_offset)
    }
}
