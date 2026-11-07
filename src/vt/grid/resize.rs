use super::Grid;
use super::cell::Cell;

impl Grid {
    /// Resizes the screen. Rows are where the content moves: see `grow_rows` and `shrink_rows`.
    /// Columns keep their old behaviour: rows are cut or padded at the right edge, nothing reflows.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(1);
        let new_rows = new_rows.max(1);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }
        self.scroll_mixed = true;
        if new_rows > self.rows {
            self.grow_rows(new_rows);
        } else if new_rows < self.rows {
            self.shrink_rows(new_rows);
        }
        for line in &mut self.lines {
            line.resize(new_cols, Cell::default());
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.scroll_left = 0;
        self.scroll_right = new_cols.saturating_sub(1);
        self.cursor.x = self.cursor.x.min(new_cols.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(new_rows.saturating_sub(1));
    }

    /// Grows the screen to `new_rows`. The newest history rows come back onto the top of the
    /// screen, so the content keeps its place above the cursor. When the cursor sat on the bottom
    /// row, the rows history cannot fill go on top too, so the content stays bottom-anchored with
    /// no blank gap below it; otherwise they go below, as tmux does. Marks on the rows that moved
    /// are renumbered with them.
    fn grow_rows(&mut self, new_rows: usize) {
        let added = new_rows - self.rows;
        let was_bottom = self.cursor.y + 1 == self.rows;
        let pulled = if self.moves_history() {
            added.min(self.history.len())
        } else {
            0
        };
        // The alternate screen is drawn by the app, which redraws: its new rows go below, as before.
        let pad_top = if was_bottom && !self.alternate_screen {
            added - pulled
        } else {
            0
        };
        let pad_bottom = added - pulled - pad_top;

        let start = self.history.len() - pulled;
        let (pulled_lines, pulled_wrapped): (Vec<Vec<Cell>>, Vec<bool>) = self
            .history
            .drain(start..)
            .map(|row| (row.to_cells(self.cols), row.wrapped))
            .unzip();

        let mut lines: Vec<Vec<Cell>> = std::iter::repeat_with(Vec::new).take(pad_top).collect();
        lines.extend(pulled_lines);
        lines.append(&mut self.lines);
        lines.extend(std::iter::repeat_with(Vec::new).take(pad_bottom));
        self.lines = lines;

        let mut wrapped = vec![false; pad_top];
        wrapped.extend(pulled_wrapped);
        wrapped.append(&mut self.wrapped);
        wrapped.resize(new_rows, false);
        self.wrapped = wrapped;

        self.cursor.y += pad_top + pulled;
        self.scrolled_off -= pulled;
        if pad_top > 0 {
            // Rows below the pad are numbered `pad_top` higher than before: the pad has no number.
            let first = self.scrolled_off;
            for mark in self.prompts.iter_mut().chain(self.outputs.iter_mut()) {
                if *mark >= first {
                    *mark += pad_top;
                }
            }
        }
    }

    /// Shrinks the screen to `new_rows`. When the cursor would fall below the new bottom, the top
    /// rows it would push off go into history, so the cursor keeps its row; otherwise rows are cut
    /// from the bottom. With no scrollback the pushed rows are only counted, as scrolling counts them.
    fn shrink_rows(&mut self, new_rows: usize) {
        let pushed = if self.alternate_screen {
            0
        } else {
            (self.cursor.y + 1).saturating_sub(new_rows)
        };
        for y in 0..pushed {
            self.keep_in_history(y);
        }
        self.lines.drain(..pushed);
        self.wrapped.drain(..pushed);
        self.cursor.y -= pushed;
        self.lines.truncate(new_rows);
        self.wrapped.truncate(new_rows);
    }
}
