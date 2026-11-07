//! Cursor and scroll motions: the `Motion` commands, word movement over the whole history, and
//! character jumps within a line.
use crate::vt::grid::Grid;

use super::{CopyState, PaneSize, SelectionKind};

const WORD: usize = 4;
const PAGE: usize = 15;

/// A cursor or scroll move. tmux's `cursor-*`, `*-line`, `*-word`, page and history commands map
/// onto these, and so do the keys of the default tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    BackToIndentation,
    /// `next-word` and `next-space` (`big`: the blank-separated kind of word).
    NextWord {
        big: bool,
    },
    /// `previous-word` and `previous-space`.
    PreviousWord {
        big: bool,
    },
    /// `next-word-end` and `next-space-end`.
    NextWordEnd {
        big: bool,
    },
    /// mpx's own `w` and `b` (and emacs `M-f`, `M-b`): four columns along the line.
    FourForward,
    FourBack,
    TopLine,
    MiddleLine,
    BottomLine,
    HistoryTop,
    HistoryBottom,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    ScrollUp,
    ScrollDown,
    /// `goto-line n`: line `n` of the history, counted from its top (the oldest line is 0).
    GotoLine(usize),
    /// `jump-forward` / `jump-backward` to a character of the cursor's line; `to` stops just short
    /// of it (`jump-to-forward`, `jump-to-backward`).
    Jump {
        forward: bool,
        to: bool,
        ch: char,
    },
}

/// How a character groups with its neighbours for word motions: blanks separate, `-_@` separate
/// words (tmux's default `word-separators`), anything else is a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Blank,
    Separator,
    Word,
}

fn char_class(c: char, big: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if big {
        CharClass::Word
    } else if matches!(c, '-' | '_' | '@') {
        CharClass::Separator
    } else {
        CharClass::Word
    }
}

/// A cell position: column and absolute line.
type Pos = (usize, usize);

/// The whole history and screen as one run of cells, lines read left to right, for word motions.
struct Cells<'a> {
    grid: &'a Grid,
    cols: usize,
    lines: usize,
}

impl Cells<'_> {
    fn at(&self, (x, y): Pos) -> char {
        self.grid
            .line_cells(y)
            .and_then(|cells| cells.get(x).map(|c| c.c))
            .unwrap_or(' ')
    }

    fn class(&self, p: Pos, big: bool) -> CharClass {
        char_class(self.at(p), big)
    }

    fn next(&self, (x, y): Pos) -> Option<Pos> {
        if x + 1 < self.cols {
            Some((x + 1, y))
        } else if y + 1 < self.lines {
            Some((0, y + 1))
        } else {
            None
        }
    }

    fn prev(&self, (x, y): Pos) -> Option<Pos> {
        if x > 0 {
            Some((x - 1, y))
        } else if y > 0 {
            Some((self.cols.saturating_sub(1), y - 1))
        } else {
            None
        }
    }
}

/// The start of the next word (vim `w`): past the current run, then past the blanks.
fn next_word(w: &Cells, mut p: Pos, big: bool) -> Pos {
    let start = w.class(p, big);
    if start != CharClass::Blank {
        loop {
            let Some(q) = w.next(p) else { return p };
            p = q;
            if w.class(p, big) != start {
                break;
            }
        }
    }
    while w.class(p, big) == CharClass::Blank {
        let Some(q) = w.next(p) else { return p };
        p = q;
    }
    p
}

/// Skips blanks from `p` in direction `step`, then runs to the far end of the word it lands in.
fn word_run_end(
    w: &Cells,
    mut p: Pos,
    big: bool,
    step: impl Fn(&Cells, Pos) -> Option<Pos>,
) -> Pos {
    let Some(q) = step(w, p) else { return p };
    p = q;
    while w.class(p, big) == CharClass::Blank {
        let Some(q) = step(w, p) else { return p };
        p = q;
    }
    let class = w.class(p, big);
    while let Some(q) = step(w, p) {
        if w.class(q, big) != class {
            break;
        }
        p = q;
    }
    p
}

/// The last character of the next word (vim `e`).
fn next_word_end(w: &Cells, p: Pos, big: bool) -> Pos {
    word_run_end(w, p, big, |w, p| w.next(p))
}

/// The start of the previous word (vim `b`).
fn previous_word(w: &Cells, p: Pos, big: bool) -> Pos {
    word_run_end(w, p, big, |w, p| w.prev(p))
}

/// Moves the cursor to a word position found by `step`, scrolling only when it is off screen.
fn word_move(
    cs: &mut CopyState,
    grid: Option<&Grid>,
    rows: usize,
    step: impl Fn(&Cells, Pos) -> Pos,
) {
    let Some(g) = grid else { return };
    let cells = Cells {
        grid: g,
        cols: g.cols,
        lines: g.history_len() + g.rows,
    };
    let (x, y) = step(&cells, (cs.cursor_x, cs.cursor_abs_y(g)));
    cs.scroll_to_match(x, y, g.history_len(), rows);
}

/// The character `ch` on the cursor's line, forward or backward from the cursor.
fn jump(cs: &mut CopyState, grid: Option<&Grid>, forward: bool, to: bool, ch: char) {
    let Some(g) = grid else { return };
    let Some(cells) = g.line_cells(cs.cursor_abs_y(g)) else {
        return;
    };
    let found = if forward {
        (cs.cursor_x + 1..cells.len()).find(|&x| cells[x].c == ch)
    } else {
        (0..cs.cursor_x).rev().find(|&x| cells[x].c == ch)
    };
    if let Some(x) = found {
        cs.cursor_x = match (forward, to) {
            (true, false) | (false, false) => x,
            (true, true) => x - 1,
            (false, true) => x + 1,
        };
    }
}

/// Moves the view one history line up, while there is history left.
fn scroll_up_one(cs: &mut CopyState, hist_len: usize) {
    if cs.scroll_offset < hist_len {
        cs.scroll_offset += 1;
    }
}

/// Moves the view one history line down, towards the live screen.
fn scroll_down_one(cs: &mut CopyState) {
    cs.scroll_offset = cs.scroll_offset.saturating_sub(1);
}

/// Moves the cursor to the first non-blank cell of its line (vim `^`).
fn back_to_indentation(cs: &mut CopyState, grid: Option<&Grid>) {
    if let Some(g) = grid {
        let abs_y = cs.cursor_abs_y(g);
        cs.cursor_x = g
            .line_cells(abs_y)
            .and_then(|cells| cells.iter().position(|c| !c.c.is_whitespace()))
            .unwrap_or(0);
    }
}

/// Moves the cursor to `line` (absolute), scrolling only when it is off screen.
fn goto_line(cs: &mut CopyState, grid: Option<&Grid>, line: usize, pane_h: usize) {
    if let Some(g) = grid {
        let total = g.history_len() + g.rows;
        let abs_y = line.min(total.saturating_sub(1));
        cs.scroll_to_match(cs.cursor_x, abs_y, g.history_len(), pane_h);
    }
}

/// Selects the run of same-class characters under the cursor, on its line.
pub(super) fn select_word(cs: &mut CopyState, grid: Option<&Grid>) {
    let Some(g) = grid else { return };
    let abs_y = cs.cursor_abs_y(g);
    let Some(cells) = g.line_cells(abs_y) else {
        return;
    };
    let class = |x: usize| char_class(cells.get(x).map_or(' ', |c| c.c), false);
    let x = cs.cursor_x.min(cells.len().saturating_sub(1));
    let wanted = class(x);
    let mut start = x;
    while start > 0 && class(start - 1) == wanted {
        start -= 1;
    }
    let mut end = x;
    while end + 1 < cells.len() && class(end + 1) == wanted {
        end += 1;
    }
    cs.mode = SelectionKind::Character;
    cs.anchor_x = start;
    cs.anchor_abs_y = abs_y;
    cs.cursor_x = end;
}

fn move_up(cs: &mut CopyState, hist_len: usize) {
    if cs.cursor_y > 0 {
        cs.cursor_y -= 1;
    } else {
        scroll_up_one(cs, hist_len);
    }
}

fn move_down(cs: &mut CopyState, pane_h: usize) {
    if cs.cursor_y + 1 < pane_h {
        cs.cursor_y += 1;
    } else {
        scroll_down_one(cs);
    }
}

pub(super) fn apply_motion(
    cs: &mut CopyState,
    motion: Motion,
    grid: Option<&Grid>,
    pane: PaneSize,
) {
    let (pane_w, pane_h) = (pane.0 as usize, pane.1 as usize);
    let last_col = pane_w.saturating_sub(1);
    let hist_len = grid.map_or(0, Grid::history_len);
    let half = (pane_h / 2).max(1);
    match motion {
        Motion::Left => cs.cursor_x = cs.cursor_x.saturating_sub(1),
        Motion::Right => cs.cursor_x = (cs.cursor_x + 1).min(last_col),
        Motion::Up => move_up(cs, hist_len),
        Motion::Down => move_down(cs, pane_h),
        Motion::LineStart => cs.cursor_x = 0,
        Motion::LineEnd => cs.cursor_x = last_col,
        Motion::BackToIndentation => back_to_indentation(cs, grid),
        Motion::NextWord { big } => word_move(cs, grid, pane_h, |w, p| next_word(w, p, big)),
        Motion::PreviousWord { big } => {
            word_move(cs, grid, pane_h, |w, p| previous_word(w, p, big));
        }
        Motion::NextWordEnd { big } => {
            word_move(cs, grid, pane_h, |w, p| next_word_end(w, p, big));
        }
        Motion::FourForward => cs.cursor_x = (cs.cursor_x + WORD).min(last_col),
        Motion::FourBack => cs.cursor_x = cs.cursor_x.saturating_sub(WORD),
        Motion::TopLine => cs.cursor_y = 0,
        Motion::MiddleLine => cs.cursor_y = (pane_h / 2).min(pane_h.saturating_sub(1)),
        Motion::BottomLine => cs.cursor_y = pane_h.saturating_sub(1),
        Motion::PageUp => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        Motion::PageDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        Motion::HalfPageUp => cs.scroll_offset = (cs.scroll_offset + half).min(hist_len),
        Motion::HalfPageDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(half),
        Motion::ScrollUp => scroll_up_one(cs, hist_len),
        Motion::ScrollDown => scroll_down_one(cs),
        Motion::HistoryTop => {
            cs.scroll_offset = hist_len;
            cs.cursor_y = 0;
            cs.cursor_x = 0;
        }
        Motion::HistoryBottom => {
            cs.scroll_offset = 0;
            cs.cursor_y = pane_h.saturating_sub(1);
        }
        Motion::GotoLine(line) => goto_line(cs, grid, line, pane_h),
        Motion::Jump { forward, to, ch } => jump(cs, grid, forward, to, ch),
    }
}
