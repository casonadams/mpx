use super::cell::{Cell, CellStyle};
use super::{Cursor, Grid, SavedCursor};

impl Grid {
    /// Whether the alternate screen is shown.
    pub fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    /// Shows the alternate screen or the primary one (DEC modes 47, 1047 and 1049). Each keeps
    /// its own rows; switching to the screen already shown changes nothing.
    pub fn set_alternate_screen(&mut self, on: bool) {
        if self.alternate_screen == on {
            return;
        }
        self.alternate_screen = on;
        std::mem::swap(&mut self.lines, &mut self.hidden_lines);
        std::mem::swap(&mut self.wrapped, &mut self.hidden_wrapped);
        self.scroll_mixed = true;
    }

    /// Blanks the alternate screen's rows (shown or not), leaving the cursor where it is.
    pub fn clear_alternate_screen(&mut self) {
        let blank = self.blank();
        let (lines, wrapped) = if self.alternate_screen {
            (&mut self.lines, &mut self.wrapped)
        } else {
            (&mut self.hidden_lines, &mut self.hidden_wrapped)
        };
        for line in lines {
            line.fill(blank);
        }
        wrapped.fill(false);
    }

    /// The rows of the screen that is not shown, each with whether it wrapped at the right margin.
    pub fn hidden_screen_lines(&self) -> impl Iterator<Item = (&[Cell], bool)> {
        self.hidden_lines
            .iter()
            .map(Vec::as_slice)
            .zip(self.hidden_wrapped.iter().copied())
    }

    /// The cursor position that column `x` and row `y` of CUP address: from the margins' corner
    /// under origin mode, clamped inside them, otherwise from the screen's.
    pub fn addressed_position(&self, x: usize, y: usize) -> (usize, usize) {
        if !self.origin_mode {
            return (x, y);
        }
        let left = if self.lr_margins { self.scroll_left } else { 0 };
        let right = if self.lr_margins {
            self.scroll_right
        } else {
            self.cols.saturating_sub(1)
        };
        (
            left.saturating_add(x).min(right),
            self.scroll_top.saturating_add(y).min(self.scroll_bottom),
        )
    }

    /// Moves the cursor to the home position: the screen's corner, or the margins' under origin mode.
    pub fn home_cursor(&mut self) {
        let (x, y) = self.addressed_position(0, 0);
        self.move_cursor(x, y);
    }

    /// The visible rows, top to bottom.
    pub fn screen_lines(&self) -> impl DoubleEndedIterator<Item = &[Cell]> {
        self.lines.iter().map(Vec::as_slice)
    }

    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        self.lines.get(y).and_then(|line| line.get(x))
    }

    pub fn get_cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        self.lines.get_mut(y).and_then(|line| line.get_mut(x))
    }

    /// DECSC: saves the cursor, the rendition and origin mode.
    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            cursor: self.cursor,
            style: self.current_style,
            origin_mode: self.origin_mode,
        });
    }

    /// DECRC: restores what DECSC saved; with nothing saved, homes the cursor and resets the
    /// rendition and origin mode, as xterm does.
    pub fn restore_cursor(&mut self) {
        let saved = self.saved_cursor.unwrap_or(SavedCursor {
            cursor: Cursor {
                x: 0,
                y: 0,
                visible: self.cursor.visible,
            },
            style: CellStyle::default(),
            origin_mode: false,
        });
        self.cursor = saved.cursor;
        self.cursor.x = self.cursor.x.min(self.cols);
        self.cursor.y = self.cursor.y.min(self.rows - 1);
        self.current_style = saved.style;
        self.origin_mode = saved.origin_mode;
    }

    /// DECALN: fills the screen with `E`, resets the margins and homes the cursor.
    pub fn align_test(&mut self) {
        let e = Cell {
            c: 'E',
            ..Cell::default()
        };
        for line in &mut self.lines {
            line.fill(e);
        }
        self.wrapped.fill(false);
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.scroll_left = 0;
        self.scroll_right = self.cols - 1;
        self.origin_mode = false;
        self.cursor.x = 0;
        self.cursor.y = 0;
    }

    pub fn move_cursor(&mut self, x: usize, y: usize) {
        self.cursor.x = x.min(self.cols.saturating_sub(1));
        self.cursor.y = y.min(self.rows.saturating_sub(1));
    }

    pub fn render_plain(&self) -> String {
        let mut out = String::with_capacity(self.rows * (self.cols + 1));
        for line in &self.lines {
            let start = out.len();
            for cell in line {
                cell.push_text(&mut out);
            }
            out.truncate(start + out[start..].trim_end().len());
            out.push('\n');
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}
