use std::borrow::Cow;

use super::Grid;
use super::cell::{BLANK, Cell, CellStyle};

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
    /// The row wrapped at the right margin: its text continues on the next row.
    pub(super) wrapped: bool,
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
            wrapped: false,
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

impl Grid {
    /// Whether row changes may move rows to or from history: never on the alternate screen, whose
    /// apps redraw, and never without scrollback.
    pub(super) fn moves_history(&self) -> bool {
        !self.alternate_screen && self.history_limit > 0
    }

    /// Copies screen row `y`, which just scrolled off the top, into history without its trailing
    /// default blanks, so short lines cost their length rather than the screen width. The screen
    /// row itself is reused.
    pub(super) fn keep_in_history(&mut self, y: usize) {
        self.scrolled_off += 1;
        if self.history_limit == 0 {
            return;
        }
        if self.history.len() >= self.history_limit {
            self.history.pop_front();
        }
        let mut row = HistoryRow::from_cells(&self.lines[y]);
        row.wrapped = self.wrapped[y];
        self.history.push_back(row);
    }

    /// Caps scrollback at `limit` rows, dropping the oldest beyond it; 0 keeps none.
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        let excess = self.history.len().saturating_sub(limit);
        self.history.drain(..excess);
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Whether line `abs_line` (history or live screen) wrapped at the right margin.
    pub fn line_wrapped(&self, abs_line: usize) -> bool {
        let total_hist = self.history.len();
        match abs_line.checked_sub(total_hist) {
            Some(y) => self.wrapped.get(y).copied().unwrap_or(false),
            None => self.history.get(abs_line).is_some_and(|row| row.wrapped),
        }
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

    /// Returns the text of line `abs_line` (history or live screen), trailing blanks trimmed.
    pub fn line_text(&self, abs_line: usize) -> Option<String> {
        let cells = self.line_cells(abs_line)?;
        let mut text: String = cells.iter().map(|c| c.c).collect();
        text.truncate(text.trim_end().len());
        Some(text)
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
}
