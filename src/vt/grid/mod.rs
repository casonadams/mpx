mod cell;
mod edit;
mod history;
mod marks;
mod resize;
mod screen;
mod scroll;

use std::collections::VecDeque;

pub use cell::{Cell, CellStyle, Color, Underline};
pub use history::HistoryRow;

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

/// A shell-integration mark (OSC 133): where a prompt starts (`A`) or command output starts (`C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMark {
    Prompt,
    Output,
}

#[derive(Debug, Clone)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    /// One vector of `cols` cells per screen row, so scrolling moves rows instead of cells.
    lines: Vec<Vec<Cell>>,
    /// Per screen row: the row wrapped at the right margin, so its text continues on the next row.
    wrapped: Vec<bool>,
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
    /// Rows that have ever scrolled off the top, kept in history or not: numbers the shell marks.
    scrolled_off: usize,
    /// Shell marks by row number, oldest first; a mark is kept while its row is in history.
    prompts: VecDeque<usize>,
    outputs: VecDeque<usize>,
    scrolled: Option<ScrollHint>,
    /// Scrolls of different regions (or a resize) happened since the hint was last taken.
    scroll_mixed: bool,
    /// The alternate screen is shown: its rows never move to or from history on a resize.
    alternate_screen: bool,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            lines: vec![vec![Cell::default(); cols]; rows],
            wrapped: vec![false; rows],
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
            scrolled_off: 0,
            prompts: VecDeque::new(),
            outputs: VecDeque::new(),
            scrolled: None,
            scroll_mixed: false,
            alternate_screen: false,
        }
    }

    /// The screen as it is after a terminal reset (RIS) at the same size. Scrollback stays, with
    /// its limit and the count of rows scrolled off, so its rows keep their numbers.
    pub fn reset(&mut self) {
        let fresh = Self::new(self.cols, self.rows);
        *self = Self {
            history: std::mem::take(&mut self.history),
            history_limit: self.history_limit,
            scrolled_off: self.scrolled_off,
            ..fresh
        };
    }
}
