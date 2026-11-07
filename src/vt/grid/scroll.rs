use super::{Grid, ScrollHint};

impl Grid {
    /// DECLRMM: turning it off restores full-width margins.
    pub fn set_lr_margins_mode(&mut self, on: bool) {
        self.lr_margins = on;
        if !on {
            self.scroll_left = 0;
            self.scroll_right = self.cols.saturating_sub(1);
        }
    }

    /// DECSLRM (1-based, inclusive; `None` for a default), ignored unless DECLRMM is set. Homes the
    /// cursor, as DECSTBM does.
    pub fn set_lr_margins(&mut self, left: Option<usize>, right: Option<usize>) {
        if !self.lr_margins {
            return;
        }
        let left = left.unwrap_or(1).max(1) - 1;
        let right = right.unwrap_or(self.cols).clamp(1, self.cols) - 1;
        if left < right {
            self.scroll_left = left;
            self.scroll_right = right;
        }
        self.move_cursor(0, 0);
    }

    /// Left/right margins narrower than the screen: scrolling moves only the columns between them.
    fn narrow_margins(&self) -> Option<(usize, usize)> {
        let full = self.scroll_left == 0 && self.scroll_right + 1 >= self.cols;
        (self.lr_margins && !full).then_some((self.scroll_left, self.scroll_right))
    }

    /// Scrolls the columns `left..=right` of rows `top..=bottom` by `lines` (up when positive),
    /// blanking what scrolls in. Nothing enters history: only part of each row moved.
    fn scroll_columns(&mut self, (left, right): (usize, usize), lines: isize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = lines.unsigned_abs().min(bottom - top + 1);
        self.scroll_mixed = true;
        self.wrapped[top..=bottom].fill(false);
        let blank = self.blank();
        let rows: Vec<usize> = if lines > 0 {
            (top..=bottom).collect()
        } else {
            (top..=bottom).rev().collect()
        };
        for &y in &rows {
            let from = if lines > 0 {
                y.checked_add(n).filter(|&f| f <= bottom)
            } else {
                y.checked_sub(n).filter(|&f| f >= top)
            };
            match from {
                Some(f) => {
                    let moved = self.lines[f][left..=right].to_vec();
                    self.lines[y][left..=right].copy_from_slice(&moved);
                }
                None => self.lines[y][left..=right].fill(blank),
            }
        }
    }

    pub fn scroll_up(&mut self, count: usize) {
        self.scroll_region(count, true);
    }

    /// Scrolls the lines of the scroll region by `count` rows, up or down. Scrolling up sends the
    /// rows leaving the top of a full screen to history and blanks rows entering at the bottom;
    /// scrolling down blanks rows entering at the top.
    fn scroll_region(&mut self, count: usize, up: bool) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }
        if let Some(columns) = self.narrow_margins() {
            let lines = count as isize;
            self.scroll_columns(columns, if up { lines } else { -lines });
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = count.min(bottom - top + 1);
        if up {
            self.note_scroll(top, bottom, n as isize);
            self.lines[top..=bottom].rotate_left(n);
            self.wrapped[top..=bottom].rotate_left(n);
        } else {
            self.note_scroll(top, bottom, -(n as isize));
            self.lines[top..=bottom].rotate_right(n);
            self.wrapped[top..=bottom].rotate_right(n);
        }
        let blank = self.blank();
        let entering = if up {
            bottom + 1 - n..=bottom
        } else {
            top..=top + n - 1
        };
        for y in entering {
            if up && top == 0 {
                self.keep_in_history(y);
            }
            self.lines[y].fill(blank);
            self.wrapped[y] = false;
        }
    }

    fn note_scroll(&mut self, top: usize, bottom: usize, lines: isize) {
        match &mut self.scrolled {
            Some(hint) if hint.top == top && hint.bottom == bottom => hint.lines += lines,
            Some(_) => self.scroll_mixed = true,
            None => {
                self.scrolled = Some(ScrollHint { top, bottom, lines });
            }
        }
    }

    /// The net scroll since the last call, if it was a single region moving by a nonzero amount.
    pub fn take_scroll(&mut self) -> Option<ScrollHint> {
        let hint = self.scrolled.take();
        let mixed = std::mem::take(&mut self.scroll_mixed);
        hint.filter(|h| !mixed && h.lines != 0)
    }

    pub fn scroll_down(&mut self, count: usize) {
        self.scroll_region(count, false);
    }

    pub fn delete_lines(&mut self, count: usize) {
        if self.cursor.y < self.scroll_top || self.cursor.y > self.scroll_bottom {
            return;
        }
        let saved_top = self.scroll_top;
        self.scroll_top = self.cursor.y;
        self.scroll_up(count);
        self.scroll_top = saved_top;
    }

    pub fn insert_lines(&mut self, count: usize) {
        if self.cursor.y < self.scroll_top || self.cursor.y > self.scroll_bottom {
            return;
        }
        let saved_top = self.scroll_top;
        self.scroll_top = self.cursor.y;
        self.scroll_down(count);
        self.scroll_top = saved_top;
    }
}
