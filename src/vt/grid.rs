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
    pub cells: Vec<Cell>,
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
        let cells = vec![Cell::default(); cols * rows];
        Self {
            cols,
            rows,
            cells,
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
        let mut new_cells = vec![Cell::default(); new_cols * new_rows];
        let copy_cols = self.cols.min(new_cols);
        let copy_rows = self.rows.min(new_rows);
        for y in 0..copy_rows {
            for x in 0..copy_cols {
                new_cells[y * new_cols + x] = self.cells[y * self.cols + x];
            }
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.cells = new_cells;
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.cursor.x = self.cursor.x.min(new_cols.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(new_rows.saturating_sub(1));
    }


    #[inline]
    fn index(&self, x: usize, y: usize) -> usize {
        y * self.cols + x
    }

    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        if x < self.cols && y < self.rows {
            Some(&self.cells[self.index(x, y)])
        } else {
            None
        }
    }

    pub fn get_cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.cols && y < self.rows {
            let idx = self.index(x, y);
            Some(&mut self.cells[idx])
        } else {
            None
        }
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
            if live_y < self.rows {
                let start = live_y * self.cols;
                let end = start + self.cols;
                Some(self.cells[start..end].to_vec())
            } else {
                None
            }
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

    pub fn scroll_up(&mut self, count: usize) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }

        let top = self.scroll_top;
        let bottom = self.scroll_bottom;
        let cols = self.cols;
        let window_height = bottom - top + 1;
        let actual_count = count.min(window_height);

        if top == 0 {
            for row in 0..actual_count {
                let start = row * cols;
                let end = start + cols;
                let mut line = if self.history.len() >= self.history_limit {
                    let mut recycled = self.history.pop_front().unwrap_or_default();
                    recycled.clear();
                    recycled
                } else {
                    Vec::with_capacity(cols)
                };
                line.extend_from_slice(&self.cells[start..end]);
                self.history.push_back(line);
            }
        }

        if actual_count < window_height {
            let src_start = (top + actual_count) * cols;
            let src_end = (bottom + 1) * cols;
            let dst_start = top * cols;
            self.cells.copy_within(src_start..src_end, dst_start);
        }

        let clear_start = (bottom + 1 - actual_count) * cols;
        let clear_end = (bottom + 1) * cols;
        self.cells[clear_start..clear_end].fill(Cell::default());
    }

    pub fn scroll_down(&mut self, count: usize) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }

        let top = self.scroll_top;
        let bottom = self.scroll_bottom;
        let cols = self.cols;
        let window_height = bottom - top + 1;
        let actual_count = count.min(window_height);

        if actual_count < window_height {
            let src_start = top * cols;
            let src_end = (bottom + 1 - actual_count) * cols;
            let dst_start = (top + actual_count) * cols;
            self.cells.copy_within(src_start..src_end, dst_start);
        }

        let clear_start = top * cols;
        let clear_end = (top + actual_count) * cols;
        self.cells[clear_start..clear_end].fill(Cell::default());
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

    pub fn clear_line(&mut self, mode: usize) {
        let y = self.cursor.y;
        let cols = self.cols;
        match mode {
            0 => {
                // Clear from cursor to end of line
                for x in self.cursor.x..cols {
                    let idx = self.index(x, y);
                    self.cells[idx] = Cell::default();
                }
            }
            1 => {
                // Clear from beginning of line to cursor
                let limit = (self.cursor.x + 1).min(cols);
                for x in 0..limit {
                    let idx = self.index(x, y);
                    self.cells[idx] = Cell::default();
                }
            }
            2 => {
                // Clear entire line
                for x in 0..cols {
                    let idx = self.index(x, y);
                    self.cells[idx] = Cell::default();
                }
            }
            _ => {}
        }
    }

    pub fn clear_screen(&mut self, mode: usize) {
        match mode {
            0 => {
                // Clear from cursor to end of screen
                self.clear_line(0);
                for y in (self.cursor.y + 1)..self.rows {
                    for x in 0..self.cols {
                        let idx = self.index(x, y);
                        self.cells[idx] = Cell::default();
                    }
                }
            }
            1 => {
                // Clear from beginning of screen to cursor
                for y in 0..self.cursor.y {
                    for x in 0..self.cols {
                        let idx = self.index(x, y);
                        self.cells[idx] = Cell::default();
                    }
                }
                self.clear_line(1);
            }
            2 | 3 => {
                // Clear entire screen
                for cell in &mut self.cells {
                    *cell = Cell::default();
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
        for y in 0..self.rows {
            let mut line = String::with_capacity(self.cols);
            for x in 0..self.cols {
                let cell = &self.cells[self.index(x, y)];
                line.push(cell.c);
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_basic_write() {
        let mut grid = Grid::new(10, 4);
        for c in "hello".chars() {
            grid.put_char(c);
        }
        assert_eq!(grid.cursor.x, 5);
        assert_eq!(grid.cursor.y, 0);
        assert!(grid.render_plain().starts_with("hello"));
    }

    #[test]
    fn test_grid_newline_scroll() {
        let mut grid = Grid::new(10, 2);
        for c in "line1".chars() {
            grid.put_char(c);
        }
        grid.carriage_return();
        grid.newline();
        for c in "line2".chars() {
            grid.put_char(c);
        }
        grid.carriage_return();
        grid.newline();
        for c in "line3".chars() {
            grid.put_char(c);
        }
        let rendered = grid.render_plain();
        assert!(!rendered.contains("line1"));
        assert!(rendered.contains("line2"));
        assert!(rendered.contains("line3"));
    }

    #[test]
    fn test_grid_multi_scroll_and_history() {
        let mut grid = Grid::new(10, 4);
        for i in 0..6 {
            for c in format!("row{}", i).chars() {
                grid.put_char(c);
            }
            grid.carriage_return();
            grid.newline();
        }
        assert!(grid.history.len() >= 2);
        assert_eq!(grid.history[0][0].c, 'r');
        assert_eq!(grid.history[0][3].c, '0');
    }

    #[test]
    fn test_grid_clear_screen() {
        let mut grid = Grid::new(10, 2);
        for c in "abc".chars() {
            grid.put_char(c);
        }
        grid.clear_screen(2);
        assert_eq!(grid.cursor.x, 0);
        assert_eq!(grid.cursor.y, 0);
        assert_eq!(grid.get_cell(0, 0).unwrap().c, ' ');
    }
}
