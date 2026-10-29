use std::borrow::Cow;
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
    /// Reverse video (SGR 7): the terminal swaps foreground and background when drawing.
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    /// Faint (SGR 2): the terminal draws the text dimmer than its color.
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub style: CellStyle,
}

/// A blank cell in default attributes: what a history row holds past its stored end.
const BLANK: Cell = Cell {
    c: ' ',
    style: CellStyle {
        reverse: false,
        fg: None,
        bg: None,
        bold: false,
        faint: false,
        italic: false,
        underline: false,
    },
};

/// Text of a history row: one byte per character when it is all ASCII.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowText {
    Ascii(Box<[u8]>),
    Wide(Box<[char]>),
}

/// A row that scrolled into history, stored compactly: its characters up to the last one that is
/// not a default blank, and the style of each run of equally styled cells. Typical scrollback
/// costs about a byte per character instead of a full-width row of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    text: RowText,
    /// First column and style of each run, in column order; the first run starts at column 0.
    runs: Box<[(u16, CellStyle)]>,
}

impl HistoryRow {
    pub fn from_cells(cells: &[Cell]) -> Self {
        let used = cells.iter().rposition(|c| *c != BLANK).map_or(0, |i| i + 1);
        let cells = &cells[..used];
        let text = if cells.iter().all(|c| c.c.is_ascii()) {
            RowText::Ascii(cells.iter().map(|c| c.c as u8).collect())
        } else {
            RowText::Wide(cells.iter().map(|c| c.c).collect())
        };
        let mut runs: Vec<(u16, CellStyle)> = Vec::new();
        for (x, cell) in cells.iter().enumerate() {
            if runs.last().is_none_or(|(_, style)| *style != cell.style) {
                runs.push((x as u16, cell.style));
            }
        }
        Self {
            text,
            runs: runs.into_boxed_slice(),
        }
    }

    /// Stored cells; every column past them is a default blank.
    pub fn len(&self) -> usize {
        match &self.text {
            RowText::Ascii(bytes) => bytes.len(),
            RowText::Wide(chars) => chars.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The cell at column `x`, if stored.
    pub fn cell(&self, x: usize) -> Option<Cell> {
        let c = match &self.text {
            RowText::Ascii(bytes) => *bytes.get(x)? as char,
            RowText::Wide(chars) => *chars.get(x)?,
        };
        let run = self.runs.partition_point(|(start, _)| *start as usize <= x);
        let style = self.runs[run.saturating_sub(1)].1;
        Some(Cell { c, style })
    }

    /// The row as cells, padded with default blanks to at least `width`.
    pub fn to_cells(&self, width: usize) -> Vec<Cell> {
        let mut cells: Vec<Cell> = (0..self.len()).filter_map(|x| self.cell(x)).collect();
        if cells.len() < width {
            cells.resize(width, BLANK);
        }
        cells
    }
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

/// How far the content of rows `top..=bottom` moved since it was last taken: `lines` up when
/// positive, down when negative. Lets a renderer scroll the terminal instead of repainting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollHint {
    pub top: usize,
    pub bottom: usize,
    pub lines: isize,
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
    /// Left/right margins (DECSLRM), honoured while `lr_margins` (DECLRMM, mode 69) is set.
    pub lr_margins: bool,
    pub scroll_left: usize,
    pub scroll_right: usize,
    pub current_style: CellStyle,
    /// Rows that scrolled off the top, oldest first, each without its trailing default blanks.
    pub history: VecDeque<HistoryRow>,
    pub history_limit: usize,
    scrolled: Option<ScrollHint>,
    /// Scrolls of different regions (or a resize) happened since the hint was last taken.
    scroll_mixed: bool,
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
            lr_margins: false,
            scroll_left: 0,
            scroll_right: cols.saturating_sub(1),
            current_style: CellStyle::default(),
            history: VecDeque::new(),
            history_limit: 100_000,
            scrolled: None,
            scroll_mixed: false,
        }
    }
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(1);
        let new_rows = new_rows.max(1);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }
        self.scroll_mixed = true;
        self.lines.resize_with(new_rows, Vec::new);
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
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn get_cell_scrolled(&self, x: usize, y: usize, scroll_offset: usize) -> Option<Cell> {
        if x >= self.cols || y >= self.rows {
            return None;
        }

        if scroll_offset == 0 || self.history.is_empty() {
            return self.get_cell(x, y).copied();
        }

        let total_hist = self.history.len();
        let scroll_offset = scroll_offset.min(total_hist);
        let top_virtual_idx = total_hist.saturating_sub(scroll_offset);
        let virtual_y = top_virtual_idx + y;

        if virtual_y < total_hist {
            self.history
                .get(virtual_y)
                .map(|row| row.cell(x).unwrap_or(BLANK))
        } else {
            let live_y = virtual_y - total_hist;
            self.get_cell(x, live_y).copied()
        }
    }

    pub fn abs_line(&self, screen_y: usize, scroll_offset: usize) -> usize {
        let total_hist = self.history.len();
        let scroll_offset = scroll_offset.min(total_hist);
        (total_hist + screen_y).saturating_sub(scroll_offset)
    }

    /// Cells of line `abs_line` (history or live screen), at least `cols` wide.
    pub fn line_cells(&self, abs_line: usize) -> Option<Cow<'_, [Cell]>> {
        let total_hist = self.history.len();
        if abs_line >= total_hist {
            return self
                .lines
                .get(abs_line - total_hist)
                .map(|row| Cow::Borrowed(row.as_slice()));
        }
        let row = self.history.get(abs_line)?;
        Some(Cow::Owned(row.to_cells(self.cols)))
    }

    pub fn get_line_cells(&self, abs_line: usize) -> Option<Vec<Cell>> {
        self.line_cells(abs_line).map(Cow::into_owned)
    }

    /// Returns the text of line `abs_line` (history or live screen), trailing blanks trimmed.
    pub fn line_text(&self, abs_line: usize) -> Option<String> {
        let cells = self.line_cells(abs_line)?;
        Some(cells.iter().map(|c| c.c).collect::<String>().trim_end().to_string())
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
    /// Caps scrollback at `limit` rows, dropping the oldest beyond it; 0 keeps none.
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        let excess = self.history.len().saturating_sub(limit);
        self.history.drain(..excess);
    }

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
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }
        if let Some(columns) = self.narrow_margins() {
            self.scroll_columns(columns, count as isize);
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = count.min(bottom - top + 1);
        self.note_scroll(top, bottom, n as isize);
        self.lines[top..=bottom].rotate_left(n);
        let blank = self.blank();
        for y in bottom + 1 - n..=bottom {
            if top == 0 {
                self.keep_in_history(y);
            }
            self.lines[y].fill(blank);
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

    /// An erased cell: blank, in the current background color (BCE, as xterm and tmux do), so
    /// programs can paint a line's remainder by erasing it.
    fn blank(&self) -> Cell {
        Cell {
            c: ' ',
            style: CellStyle {
                bg: self.current_style.bg,
                ..CellStyle::default()
            },
        }
    }

    /// Copies screen row `y`, which just scrolled off the top, into history without its trailing
    /// default blanks, so short lines cost their length rather than the screen width. The screen
    /// row itself is reused.
    fn keep_in_history(&mut self, y: usize) {
        if self.history_limit == 0 {
            return;
        }
        if self.history.len() >= self.history_limit {
            self.history.pop_front();
        }
        self.history.push_back(HistoryRow::from_cells(&self.lines[y]));
    }

    pub fn scroll_down(&mut self, count: usize) {
        if count == 0 || self.scroll_top >= self.scroll_bottom {
            return;
        }
        if let Some(columns) = self.narrow_margins() {
            self.scroll_columns(columns, -(count as isize));
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = count.min(bottom - top + 1);
        self.note_scroll(top, bottom, -(n as isize));
        let blank = self.blank();
        self.lines[top..=bottom].rotate_right(n);
        for line in &mut self.lines[top..top + n] {
            line.fill(blank);
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
