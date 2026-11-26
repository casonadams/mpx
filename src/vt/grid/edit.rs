use super::Grid;
use super::cell::{Cell, CellStyle, WIDE_SPACER, char_width};
use crate::vt::combining::{Combining, EMOJI_PRESENTATION};

/// Keeps a wide character whole across the boundary before column `x` of `row`: a wide character
/// whose right half is gone, or a right half whose character is gone, becomes a blank in its style.
pub(super) fn heal(row: &mut [Cell], x: usize) {
    let left_wide = x > 0 && row[x - 1].is_wide();
    let spacer = row.get(x).is_some_and(Cell::is_wide_spacer);
    if left_wide && !spacer {
        row[x - 1] = blanked(row[x - 1]);
    } else if spacer && !left_wide {
        row[x] = blanked(row[x]);
    }
}

/// `heal` on both sides of column `x`, which was just overwritten.
#[cold]
fn heal_around(row: &mut [Cell], x: usize) {
    heal(row, x);
    heal(row, x + 1);
}

fn blanked(cell: Cell) -> Cell {
    Cell {
        c: ' ',
        combining: None,
        ..cell
    }
}

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
            combining: None,
        }
    }

    /// Prints `c` at the cursor in its Unicode width: a wide character takes two cells (the
    /// second a `WIDE_SPACER`), a zero-width one joins the cell before the cursor, and a control
    /// character is dropped.
    #[inline]
    pub fn put_char(&mut self, c: char) {
        if (' '..'\x7f').contains(&c) {
            self.put_narrow(c);
        } else {
            self.put_unicode(c);
        }
    }

    #[inline(never)]
    fn put_unicode(&mut self, c: char) {
        match char_width(c) {
            Some(1) => self.put_narrow(c),
            Some(2) => self.put_wide(c),
            Some(0) => self.join_previous(c),
            _ => {}
        }
    }

    #[inline]
    fn wrap_pending(&mut self) {
        if self.cursor.x >= self.cols {
            self.wrap_or_clamp();
        }
    }

    #[cold]
    fn wrap_or_clamp(&mut self) {
        if self.autowrap {
            self.cursor.x = 0;
            self.wrapped[self.cursor.y] = true;
            self.newline();
        } else {
            self.cursor.x = self.cols - 1;
        }
    }

    #[inline]
    fn put_narrow(&mut self, c: char) {
        self.wrap_pending();
        if self.insert_mode {
            self.insert_blanks(1);
        }
        let (x, y) = (self.cursor.x, self.cursor.y);
        let style = self.current_style;
        let row = &mut self.lines[y];
        let replaced = row[x];
        row[x] = Cell {
            c,
            style,
            combining: None,
        };
        if !replaced.is_plain() {
            heal_around(row, x);
        }
        if self.autowrap || x + 1 < self.cols {
            self.cursor.x += 1;
        }
    }

    fn put_wide(&mut self, c: char) {
        let cols = self.cols;
        if cols < 2 {
            return;
        }
        if self.cursor.x + 1 >= cols {
            if self.autowrap {
                if self.cursor.x == cols - 1 {
                    let blank = self.blank();
                    let row = &mut self.lines[self.cursor.y];
                    row[cols - 1] = blank;
                    heal(row, cols - 1);
                }
                self.cursor.x = 0;
                self.wrapped[self.cursor.y] = true;
                self.newline();
            } else {
                self.cursor.x = cols - 2;
            }
        }
        if self.insert_mode {
            self.insert_blanks(2);
        }
        let (x, y) = (self.cursor.x, self.cursor.y);
        let style = self.current_style;
        let row = &mut self.lines[y];
        let (left, right) = (row[x], row[x + 1]);
        row[x] = Cell {
            c,
            style,
            combining: None,
        };
        row[x + 1] = Cell {
            c: WIDE_SPACER,
            style,
            combining: None,
        };
        if left.is_wide_spacer() {
            heal(row, x);
        }
        if !right.is_plain() {
            heal(row, x + 2);
        }
        self.cursor.x = if self.autowrap || x + 2 < cols {
            x + 2
        } else {
            cols - 1
        };
    }

    /// Draws zero-width `c` on the character before the cursor (the last cell when a wrap is
    /// pending); dropped at the start of a row. `EMOJI_PRESENTATION` on a narrow character makes it
    /// two columns wide, as tmux does: the next cell becomes its right half and the cursor moves
    /// past it. With no column left for that half on the row, the selector is dropped and the
    /// character stays narrow.
    fn join_previous(&mut self, c: char) {
        let x = self.cursor.x.min(self.cols);
        if x == 0 {
            return;
        }
        let cols = self.cols;
        let row = &mut self.lines[self.cursor.y];
        let mut at = x - 1;
        if row[at].is_wide_spacer() && at > 0 {
            at -= 1;
        }
        let cell = row[at];
        if cell.is_wide_spacer() {
            return;
        }
        let widen = c == EMOJI_PRESENTATION && !cell.is_wide();
        if widen && x == cols {
            return;
        }
        let Some(run) = Combining::append(cell.combining, c) else {
            return;
        };
        row[at].combining = Some(run);
        if widen {
            row[x] = Cell {
                c: WIDE_SPACER,
                style: cell.style,
                combining: None,
            };
            heal(row, x + 1);
            self.cursor.x = if self.autowrap || x + 1 < cols {
                x + 1
            } else {
                cols - 1
            };
        }
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
        self.cursor.x = self.cursor.x.min(self.cols - 1).saturating_sub(1);
    }

    fn cursor_row_mut(&mut self) -> &mut [Cell] {
        &mut self.lines[self.cursor.y]
    }

    pub fn erase_chars(&mut self, count: usize) {
        let cols = self.cols;
        let start = self.cursor.x.min(cols);
        let end = start.saturating_add(count).min(cols);
        let blank = self.blank();
        let row = self.cursor_row_mut();
        row[start..end].fill(blank);
        heal(row, start);
        heal(row, end);
    }

    pub fn insert_blanks(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let blank = self.blank();
        let row = self.cursor_row_mut();
        row.copy_within(x..cols - n, x + n);
        row[x..x + n].fill(blank);
        heal(row, x);
        heal(row, x + n);
        heal(row, cols);
    }

    pub fn delete_chars(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let blank = self.blank();
        let row = self.cursor_row_mut();
        row.copy_within(x + n..cols, x);
        row[cols - n..].fill(blank);
        heal(row, x);
        heal(row, cols - n);
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
            0 => {
                row[x.min(cols)..].fill(blank);
                heal(row, x.min(cols));
            }
            1 => {
                let end = (x + 1).min(cols);
                row[..end].fill(blank);
                heal(row, end);
            }
            2 => row.fill(blank),
            _ => {}
        }
    }

    pub fn clear_screen(&mut self, mode: usize) {
        if mode == 3 {
            self.history.clear();
            return;
        }
        let blank = self.blank();
        match mode {
            0 => self.wrapped[self.cursor.y + 1..].fill(false),
            1 => self.wrapped[..self.cursor.y].fill(false),
            2 => self.wrapped.fill(false),
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
            2 => {
                for line in &mut self.lines {
                    line.fill(blank);
                }
            }
            _ => {}
        }
    }
}
