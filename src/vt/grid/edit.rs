use super::Grid;
use super::cell::{Cell, CellStyle};

impl Grid {
    /// An erased cell: blank, in the current background color (BCE, as xterm and tmux do), so
    /// programs can paint a line's remainder by erasing it.
    pub(super) fn blank(&self) -> Cell {
        Cell {
            c: ' ',
            style: CellStyle {
                bg: self.current_style.bg,
                ..CellStyle::default()
            },
        }
    }

    pub fn put_char(&mut self, c: char) {
        if self.cursor.x >= self.cols {
            self.cursor.x = 0;
            self.wrapped[self.cursor.y] = true;
            self.newline();
        }

        let x = self.cursor.x;
        let y = self.cursor.y;
        let style = self.current_style;
        if let Some(cell) = self.get_cell_mut(x, y) {
            cell.c = c;
            cell.style = style;
        }

        self.cursor.x += 1;
    }

    pub fn newline(&mut self) {
        if self.cursor.y >= self.scroll_bottom {
            self.scroll_up(1);
            self.cursor.y = self.scroll_bottom;
        } else if self.cursor.y + 1 < self.rows {
            self.cursor.y += 1;
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor.x = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor.x > 0 {
            self.cursor.x -= 1;
        }
    }

    fn cursor_row_mut(&mut self) -> &mut [Cell] {
        &mut self.lines[self.cursor.y]
    }

    pub fn erase_chars(&mut self, count: usize) {
        let cols = self.cols;
        let start = self.cursor.x.min(cols);
        let end = start.saturating_add(count).min(cols);
        let blank = self.blank();
        self.cursor_row_mut()[start..end].fill(blank);
    }

    pub fn insert_blanks(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let blank = self.blank();
        let row = self.cursor_row_mut();
        row.copy_within(x..cols - n, x + n);
        row[x..x + n].fill(blank);
    }

    pub fn delete_chars(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let blank = self.blank();
        let row = self.cursor_row_mut();
        row.copy_within(x + n..cols, x);
        row[cols - n..].fill(blank);
    }

    pub fn clear_line(&mut self, mode: usize) {
        let cols = self.cols;
        let x = self.cursor.x;
        let blank = self.blank();
        if mode == 2 {
            self.wrapped[self.cursor.y] = false;
        }
        let row = self.cursor_row_mut();
        match mode {
            0 => row[x.min(cols)..].fill(blank),
            1 => row[..(x + 1).min(cols)].fill(blank),
            2 => row.fill(blank),
            _ => {}
        }
    }

    pub fn clear_screen(&mut self, mode: usize) {
        let blank = self.blank();
        match mode {
            0 => self.wrapped[self.cursor.y + 1..].fill(false),
            1 => self.wrapped[..self.cursor.y].fill(false),
            2 | 3 => self.wrapped.fill(false),
            _ => {}
        }
        match mode {
            0 => {
                self.clear_line(0);
                for line in &mut self.lines[self.cursor.y + 1..] {
                    line.fill(blank);
                }
            }
            1 => {
                for line in &mut self.lines[..self.cursor.y] {
                    line.fill(blank);
                }
                self.clear_line(1);
            }
            2 | 3 => {
                for line in &mut self.lines {
                    line.fill(blank);
                }
                self.cursor.x = 0;
                self.cursor.y = 0;
            }
            _ => {}
        }
    }
}
