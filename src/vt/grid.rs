use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            style: CellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub x: usize,
    pub y: usize,
    pub visible: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            visible: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    /// One vector of `cols` cells per screen row, so scrolling moves rows instead of cells.
    lines: Vec<Vec<Cell>>,
    pub cursor: Cursor,
    pub saved_cursor: Option<Cursor>,
    pub scroll_top: usize,
    pub scroll_bottom: usize,
    pub current_style: CellStyle,
    pub history: VecDeque<Vec<Cell>>,
    pub history_limit: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            lines: vec![vec![Cell::default(); cols]; rows],
            cursor: Cursor::default(),
            saved_cursor: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            current_style: CellStyle::default(),
            history: VecDeque::new(),
            history_limit: 100_000,
        }
    }
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(1);
        let new_rows = new_rows.max(1);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }
        self.lines.resize_with(new_rows, Vec::new);
        for line in &mut self.lines {
            line.resize(new_cols, Cell::default());
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.cursor.x = self.cursor.x.min(new_cols.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(new_rows.saturating_sub(1));
    }

    /// The visible rows, top to bottom.
    pub fn screen_lines(&self) -> impl Iterator<Item = &[Cell]> {
        self.lines.iter().map(Vec::as_slice)
    }

    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        self.lines.get(y).and_then(|line| line.get(x))
    }

    pub fn get_cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        self.lines.get_mut(y).and_then(|line| line.get_mut(x))
    }
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn get_cell_scrolled(&self, x: usize, y: usize, scroll_offset: usize) -> Option<&Cell> {
        if x >= self.cols || y >= self.rows {
            return None;
        }

        if scroll_offset == 0 || self.history.is_empty() {
            return self.get_cell(x, y);
        }

        let total_hist = self.history.len();
        let scroll_offset = scroll_offset.min(total_hist);
        let top_virtual_idx = total_hist.saturating_sub(scroll_offset);
        let virtual_y = top_virtual_idx + y;

        if virtual_y < total_hist {
            self.history.get(virtual_y).and_then(|row| row.get(x))
        } else {
            let live_y = virtual_y - total_hist;
            self.get_cell(x, live_y)
        }
    }

    pub fn abs_line(&self, screen_y: usize, scroll_offset: usize) -> usize {
        let total_hist = self.history.len();
        let scroll_offset = scroll_offset.min(total_hist);
        (total_hist + screen_y).saturating_sub(scroll_offset)
    }

    pub fn get_line_cells(&self, abs_line: usize) -> Option<Vec<Cell>> {
        let total_hist = self.history.len();
        if abs_line < total_hist {
            self.history.get(abs_line).cloned()
        } else {
            let live_y = abs_line - total_hist;
            self.lines.get(live_y).cloned()
        }
    }

    pub fn put_char(&mut self, c: char) {
        if self.cursor.x >= self.cols {
            self.cursor.x = 0;
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

    /// Scrolls the region up by `count` rows; rows leaving a region that starts at the top of the
    /// screen move into history.
    pub fn scroll_up(&mut self, count: usize) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = count.min(bottom - top + 1);
        self.lines[top..=bottom].rotate_left(n);
        for y in bottom + 1 - n..=bottom {
            if top == 0 {
                let blank = self.blank_line();
                let gone = std::mem::replace(&mut self.lines[y], blank);
                self.history.push_back(gone);
            } else {
                self.lines[y].fill(Cell::default());
            }
        }
    }

    /// A blank row for the screen, reusing the oldest history row once history is full.
    fn blank_line(&mut self) -> Vec<Cell> {
        let mut line = if self.history.len() >= self.history_limit {
            self.history.pop_front().unwrap_or_default()
        } else {
            Vec::with_capacity(self.cols)
        };
        line.clear();
        line.resize(self.cols, Cell::default());
        line
    }

    pub fn scroll_down(&mut self, count: usize) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = count.min(bottom - top + 1);
        self.lines[top..=bottom].rotate_right(n);
        for line in &mut self.lines[top..top + n] {
            line.fill(Cell::default());
        }
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

    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    pub fn restore_cursor(&mut self) {
        if let Some(cursor) = self.saved_cursor {
            self.cursor = cursor;
        }
    }

    fn cursor_row_mut(&mut self) -> &mut [Cell] {
        &mut self.lines[self.cursor.y]
    }

    pub fn erase_chars(&mut self, count: usize) {
        let cols = self.cols;
        let start = self.cursor.x.min(cols);
        let end = start.saturating_add(count).min(cols);
        self.cursor_row_mut()[start..end].fill(Cell::default());
    }

    pub fn insert_blanks(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let row = self.cursor_row_mut();
        row.copy_within(x..cols - n, x + n);
        row[x..x + n].fill(Cell::default());
    }

    pub fn delete_chars(&mut self, count: usize) {
        let cols = self.cols;
        let x = self.cursor.x.min(cols - 1);
        let n = count.min(cols - x);
        let row = self.cursor_row_mut();
        row.copy_within(x + n..cols, x);
        row[cols - n..].fill(Cell::default());
    }

    pub fn clear_line(&mut self, mode: usize) {
        let cols = self.cols;
        let x = self.cursor.x;
        let row = self.cursor_row_mut();
        match mode {
            0 => row[x.min(cols)..].fill(Cell::default()),
            1 => row[..(x + 1).min(cols)].fill(Cell::default()),
            2 => row.fill(Cell::default()),
            _ => {}
        }
    }

    pub fn clear_screen(&mut self, mode: usize) {
        match mode {
            0 => {
                self.clear_line(0);
                for line in &mut self.lines[self.cursor.y + 1..] {
                    line.fill(Cell::default());
                }
            }
            1 => {
                for line in &mut self.lines[..self.cursor.y] {
                    line.fill(Cell::default());
                }
                self.clear_line(1);
            }
            2 | 3 => {
                for line in &mut self.lines {
                    line.fill(Cell::default());
                }
                self.cursor.x = 0;
                self.cursor.y = 0;
            }
            _ => {}
        }
    }

    pub fn move_cursor(&mut self, x: usize, y: usize) {
        self.cursor.x = x.min(self.cols.saturating_sub(1));
        self.cursor.y = y.min(self.rows.saturating_sub(1));
    }

    pub fn render_plain(&self) -> String {
        let mut out = String::with_capacity(self.rows * (self.cols + 1));
        for line in &self.lines {
            let text: String = line.iter().map(|cell| cell.c).collect();
            out.push_str(text.trim_end());
            out.push('\n');
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}
