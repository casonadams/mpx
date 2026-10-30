//! Copy (vi-style visual) mode: cursor, scroll, selection, search and yank.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::prompt::PromptState;
use super::render::SelectionRange;
use crate::vt::grid::{Cell, Grid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    None,
    Character,
    Line,
    /// `rectangle-toggle`: a selection covering the rectangle between its corners.
    Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Up,
    Down,
}

impl SearchDirection {
    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    pub fn prompt_char(self) -> char {
        match self {
            Self::Up => '?',
            Self::Down => '/',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPrompt {
    pub direction: SearchDirection,
    pub prompt: PromptState,
}

impl SearchPrompt {
    pub fn new(direction: SearchDirection) -> Self {
        Self {
            direction,
            prompt: PromptState::new(""),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CopyState {
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub scroll_offset: usize,
    pub anchor_x: usize,
    pub anchor_abs_y: usize,
    pub mode: SelectionKind,
    pub search_prompt: Option<SearchPrompt>,
    pub last_search: Option<(SearchDirection, String)>,
    /// Entered by the wheel: scrolling it back down to the live screen leaves copy mode, as
    /// tmux's `copy-mode -e` does. Entered from the keyboard, only `q` or Esc leave.
    pub wheel_exit: bool,
}

/// What a copy-mode command asks the app to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    Stay,
    /// Leaves copy mode.
    Exit,
    /// Copies the text (paste buffer and clipboard), then leaves copy mode.
    Yank(String),
    /// Copies the text and stays in copy mode.
    Copy(String),
    /// Copies the text, pipes it to the shell `command`, and leaves copy mode when `exit`.
    Pipe {
        text: String,
        command: String,
        exit: bool,
    },
    SearchFailed(String),
}

/// Pane size in cells, used to clamp cursor motion.
pub type PaneSize = (u16, u16);

/// Rows moved by one mouse-wheel notch.
pub const WHEEL_STEP: usize = 3;
const PAGE: usize = 15;
const WORD: usize = 4;

impl CopyState {
    /// Copy mode entered at the pane's cursor.
    pub fn enter(grid: Option<&Grid>) -> Self {
        let (x, y) = grid.map_or((0, 0), |g| (g.cursor.x, g.cursor.y));
        let abs_y = grid.map_or(0, |g| g.abs_line(y, 0));
        Self {
            cursor_x: x,
            cursor_y: y,
            scroll_offset: 0,
            anchor_x: x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::None,
            search_prompt: None,
            last_search: None,
            wheel_exit: false,
        }
    }

    /// Copy mode entered by a mouse press at a local cell position.
    pub fn at_point(local_x: usize, local_y: usize, grid: Option<&Grid>) -> Self {
        let abs_y = grid.map_or(local_y, |g| g.abs_line(local_y, 0));
        Self {
            cursor_x: local_x,
            cursor_y: local_y,
            scroll_offset: 0,
            anchor_x: local_x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::Character,
            search_prompt: None,
            last_search: None,
            wheel_exit: false,
        }
    }

    /// A click inside a pane already in copy mode: the cursor and selection anchor move to the
    /// cell, any selection is dropped and the scroll position stays where it was.
    pub fn click(&mut self, local_x: usize, local_y: usize, grid: Option<&Grid>) {
        self.cursor_x = local_x;
        self.cursor_y = local_y;
        self.anchor_x = local_x;
        self.anchor_abs_y = grid.map_or(local_y, |g| g.abs_line(local_y, self.scroll_offset));
        self.mode = SelectionKind::None;
    }

    /// Drag to a cell: starts a character selection from the click anchor if none is active.
    pub fn drag_to(&mut self, local_x: usize, local_y: usize) {
        if self.mode == SelectionKind::None {
            self.mode = SelectionKind::Character;
        }
        self.cursor_x = local_x;
        self.cursor_y = local_y;
    }

    /// Copy mode entered by the wheel from the pane's cursor. With scrollback the view scrolls
    /// into history; without it the cursor moves up.
    pub fn from_wheel(grid: Option<&Grid>, hist_len: usize) -> Self {
        let (x, y, abs_y) = grid.map_or((0, 0, 0), |g| {
            (g.cursor.x, g.cursor.y, g.abs_line(g.cursor.y, 0))
        });
        Self {
            cursor_x: x,
            cursor_y: if hist_len == 0 {
                y.saturating_sub(WHEEL_STEP)
            } else {
                y
            },
            scroll_offset: WHEEL_STEP.min(hist_len),
            anchor_x: x,
            anchor_abs_y: abs_y,
            mode: SelectionKind::None,
            search_prompt: None,
            last_search: None,
            wheel_exit: true,
        }
    }

    /// One wheel notch up: scroll into history while there is more of it, then move the cursor.
    pub fn wheel_up(&mut self, hist_len: usize) {
        if hist_len > 0 && self.scroll_offset < hist_len {
            self.scroll_offset = (self.scroll_offset + WHEEL_STEP).min(hist_len);
        } else {
            self.cursor_y = self.cursor_y.saturating_sub(WHEEL_STEP);
        }
    }

    /// One wheel notch down, back toward the live screen. Returns true once it shows the live
    /// screen: scrolling the wheel back down to it leaves copy mode, as in tmux.
    pub fn wheel_down(&mut self) -> bool {
        self.scroll_offset = self.scroll_offset.saturating_sub(WHEEL_STEP);
        self.scroll_offset == 0
    }

    pub fn scroll_to_match(
        &mut self,
        match_x: usize,
        match_abs_y: usize,
        total_hist: usize,
        rows: usize,
    ) {
        let rows = rows.max(1);
        let top = total_hist.saturating_sub(self.scroll_offset);
        let bot = (total_hist + rows.saturating_sub(1)).saturating_sub(self.scroll_offset);
        if match_abs_y >= top && match_abs_y <= bot {
            self.cursor_y = match_abs_y - top;
        } else {
            self.scroll_to_abs_y(match_abs_y, total_hist, rows);
        }
        self.cursor_x = match_x;
    }

    pub fn scroll_to_abs_y(&mut self, abs_y: usize, total_hist: usize, rows: usize) {
        let rows = rows.max(1);
        let target_screen_y = (rows / 2).min(rows.saturating_sub(1));
        if abs_y >= total_hist {
            self.scroll_offset = 0;
            self.cursor_y = (abs_y - total_hist).min(rows.saturating_sub(1));
        } else {
            let ideal_scroll = (total_hist + target_screen_y).saturating_sub(abs_y);
            self.scroll_offset = ideal_scroll.min(total_hist);
            let actual_top = total_hist.saturating_sub(self.scroll_offset);
            self.cursor_y = abs_y.saturating_sub(actual_top).min(rows.saturating_sub(1));
        }
    }

    pub fn selection_range(&self, grid: &Grid) -> Option<SelectionRange> {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::None => None,
            SelectionKind::Line => Some(SelectionRange::Line {
                start_abs_y: self.anchor_abs_y,
                end_abs_y: cursor_abs_y,
            }),
            SelectionKind::Rectangle => Some(SelectionRange::Rectangle {
                anchor_x: self.anchor_x,
                anchor_abs_y: self.anchor_abs_y,
                cursor_x: self.cursor_x,
                cursor_abs_y,
            }),
            SelectionKind::Character => Some(SelectionRange::Character {
                anchor_x: self.anchor_x,
                anchor_abs_y: self.anchor_abs_y,
                cursor_x: self.cursor_x,
                cursor_abs_y,
            }),
        }
    }

    pub fn cursor_abs_y(&self, grid: &Grid) -> usize {
        grid.abs_line(self.cursor_y, self.scroll_offset)
    }

    /// True when the cursor sits exactly on the anchor, i.e. nothing is selected.
    pub fn is_empty_selection(&self, grid: &Grid) -> bool {
        self.anchor_x == self.cursor_x && self.anchor_abs_y == self.cursor_abs_y(grid)
    }

    pub fn yank_text(&self, grid: &Grid) -> String {
        let cursor_abs_y = self.cursor_abs_y(grid);
        match self.mode {
            SelectionKind::Line => yank_lines(grid, self.anchor_abs_y, cursor_abs_y),
            SelectionKind::Rectangle => {
                yank_rect(grid, self.anchor_x, self.cursor_x, self.anchor_abs_y, cursor_abs_y)
            }
            SelectionKind::Character => yank_chars(grid, self, cursor_abs_y),
            SelectionKind::None => grid
                .line_cells(cursor_abs_y)
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| c.c)
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .unwrap_or_default(),
        }
    }

    fn toggle_line(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Line {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Line;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = 0;
    }

    fn toggle_char(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Character {
            self.mode = SelectionKind::None;
            return;
        }
        self.mode = SelectionKind::Character;
        self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
        self.anchor_x = self.cursor_x;
    }

    /// `rectangle-toggle`: turns a rectangle selection into a character one and back; a character
    /// selection keeps its anchor, and with no selection a rectangle one starts at the cursor.
    fn toggle_rectangle(&mut self, grid: Option<&Grid>) {
        if self.mode == SelectionKind::Rectangle {
            self.mode = SelectionKind::Character;
            return;
        }
        if self.mode != SelectionKind::Character {
            self.anchor_abs_y = grid.map_or(0, |g| self.cursor_abs_y(g));
            self.anchor_x = self.cursor_x;
        }
        self.mode = SelectionKind::Rectangle;
    }
}

fn yank_lines(grid: &Grid, anchor_abs_y: usize, cursor_abs_y: usize) -> String {
    let (min_y, max_y) = (
        anchor_abs_y.min(cursor_abs_y),
        anchor_abs_y.max(cursor_abs_y),
    );
    let mut out = String::new();
    for y in min_y..=max_y {
        if let Some(cells) = grid.line_cells(y) {
            let line: String = cells.iter().map(|c| c.c).collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn yank_chars(grid: &Grid, cs: &CopyState, cursor_abs_y: usize) -> String {
    let anchor = (cs.anchor_x, cs.anchor_abs_y);
    let cursor = (cs.cursor_x, cursor_abs_y);
    let top = anchor.1.min(cursor.1);
    let bot = anchor.1.max(cursor.1);
    let lines: Vec<Option<Vec<char>>> = (top..=bot)
        .map(|y| {
            grid.line_cells(y)
                .map(|cells| cells.iter().map(|c| c.c).collect())
        })
        .collect();
    char_selection_text(&lines, anchor, cursor)
}

/// Text of the rectangle between two corners (`rectangle-toggle`): the columns between the
/// anchor's and the cursor's, on every row between their rows. Trailing blanks of each row go.
fn yank_rect(
    grid: &Grid,
    anchor_x: usize,
    cursor_x: usize,
    anchor_abs_y: usize,
    cursor_abs_y: usize,
) -> String {
    let (from, to) = (anchor_x.min(cursor_x), anchor_x.max(cursor_x));
    let (top, bot) = (anchor_abs_y.min(cursor_abs_y), anchor_abs_y.max(cursor_abs_y));
    (top..=bot)
        .filter_map(|y| grid.line_cells(y))
        .map(|cells| {
            row_text(&cells.iter().map(|c| c.c).collect::<Vec<char>>(), from, to)
                .unwrap_or_default()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text covered by a character selection between `anchor` and `cursor` (order-independent).
pub fn char_selection_text(
    lines: &[Option<Vec<char>>],
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> String {
    let (first, last) = ordered_span(anchor, cursor);
    let (from_col, from_row) = first;
    let (to_col, to_row) = last;
    let row_count = to_row.saturating_sub(from_row) + 1;
    let mut parts: Vec<String> = Vec::new();
    for (i, row) in lines.iter().take(row_count).enumerate() {
        let Some(cells) = row else { continue };
        let text = if row_count == 1 {
            row_text(cells, from_col, to_col)
        } else if i == 0 {
            row_text(cells, from_col, cells.len().saturating_sub(1))
                .map(|s| s.trim_end().to_string())
        } else if i + 1 == row_count {
            row_text(cells, 0, to_col)
        } else {
            row_text(cells, 0, cells.len().saturating_sub(1)).map(|s| s.trim_end().to_string())
        };
        if let Some(t) = text {
            parts.push(t);
        }
    }
    parts.join("\n")
}

fn ordered_span(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Cells `from..=to` of one row as text; `None` when the span is empty.
fn row_text(cells: &[char], from: usize, to: usize) -> Option<String> {
    if from > to || from >= cells.len() {
        return None;
    }
    let end = (to + 1).min(cells.len());
    Some(cells[from..end].iter().collect())
}

fn handle_search_key(
    cs: &mut CopyState,
    mut search: SearchPrompt,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
            cs.search_prompt = None;
            CopyOutcome::Stay
        }
        (KeyCode::Enter, _) => {
            let mut query = search.prompt.text();
            if query.is_empty()
                && let Some((_, last)) = &cs.last_search
            {
                query = last.clone();
            }
            if query.is_empty() {
                cs.search_prompt = None;
                return CopyOutcome::Stay;
            }
            let dir = search.direction;
            cs.last_search = Some((dir, query.clone()));
            cs.search_prompt = None;
            execute_search(cs, &query, dir, grid, pane)
        }
        (KeyCode::Char('u'), true) => {
            search.prompt.clear();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Char('w'), true) => {
            search.prompt.delete_word();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Char(c), false) => {
            search.prompt.insert(c);
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Backspace, _) => {
            search.prompt.backspace();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Delete, _) => {
            search.prompt.delete();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Left, _) => {
            search.prompt.left();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Right, _) => {
            search.prompt.right();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::Home, _) => {
            search.prompt.home();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        (KeyCode::End, _) => {
            search.prompt.end();
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
        _ => {
            cs.search_prompt = Some(search);
            CopyOutcome::Stay
        }
    }
}

fn execute_search(
    cs: &mut CopyState,
    query: &str,
    dir: SearchDirection,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    let Some(g) = grid else {
        return CopyOutcome::Stay;
    };
    let from = (cs.cursor_x, cs.cursor_abs_y(g));
    if let Some((col, abs_y)) = find_next_match(g, query, from, dir) {
        cs.scroll_to_match(col, abs_y, g.history_len(), pane.1 as usize);
        CopyOutcome::Stay
    } else {
        CopyOutcome::SearchFailed(query.to_string())
    }
}

pub fn line_matches(line: &str, query: &str, smart_case: bool) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let line_chars: Vec<char> = line.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    if line_chars.len() < query_chars.len() {
        return Vec::new();
    }
    let mut cols = Vec::new();
    let q_len = query_chars.len();
    for i in 0..=(line_chars.len() - q_len) {
        let window = &line_chars[i..i + q_len];
        let matches = if smart_case {
            window
                .iter()
                .zip(&query_chars)
                .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
        } else {
            window == query_chars.as_slice()
        };
        if matches {
            cols.push(i);
        }
    }
    cols
}

pub fn find_next_match(
    grid: &Grid,
    query: &str,
    from: (usize, usize),
    direction: SearchDirection,
) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let total_lines = grid.history_len() + grid.rows;
    if total_lines == 0 {
        return None;
    }
    let query_chars: Vec<char> = query.chars().collect();
    let smart_case = query_chars.iter().all(|c| !c.is_uppercase());
    let from = (from.0, from.1.min(total_lines.saturating_sub(1)));
    match direction {
        SearchDirection::Down => find_forward(grid, &query_chars, from, total_lines, smart_case),
        SearchDirection::Up => find_backward(grid, &query_chars, from, total_lines, smart_case),
    }
}

fn trimmed_cell_len(cells: &[Cell]) -> usize {
    cells
        .iter()
        .rposition(|c| !c.c.is_whitespace())
        .map_or(0, |idx| idx + 1)
}

fn cell_match(a: char, b: char, smart_case: bool) -> bool {
    if smart_case {
        a.to_lowercase().eq(b.to_lowercase())
    } else {
        a == b
    }
}

fn find_cell_match(
    cells: &[Cell],
    query: &[char],
    smart_case: bool,
    min_col: Option<usize>,
) -> Option<usize> {
    let len = trimmed_cell_len(cells);
    if len < query.len() {
        return None;
    }
    let start = min_col.map_or(0, |c| c + 1);
    if start + query.len() > len {
        return None;
    }
    (start..=(len - query.len())).find(|&i| {
        query
            .iter()
            .enumerate()
            .all(|(k, &qc)| cell_match(cells[i + k].c, qc, smart_case))
    })
}

fn rfind_cell_match(
    cells: &[Cell],
    query: &[char],
    smart_case: bool,
    max_col: Option<usize>,
) -> Option<usize> {
    let len = trimmed_cell_len(cells);
    if len < query.len() {
        return None;
    }
    let max_i = match max_col {
        Some(max) => {
            if max == 0 {
                return None;
            }
            (max - 1).min(len - query.len())
        }
        None => len - query.len(),
    };
    (0..=max_i).rev().find(|&i| {
        query
            .iter()
            .enumerate()
            .all(|(k, &qc)| cell_match(cells[i + k].c, qc, smart_case))
    })
}

fn find_forward(
    grid: &Grid,
    query: &[char],
    (from_x, from_abs_y): (usize, usize),
    total_lines: usize,
    smart_case: bool,
) -> Option<(usize, usize)> {
    if let Some(cells) = grid.line_cells(from_abs_y)
        && let Some(col) = find_cell_match(&cells, query, smart_case, Some(from_x))
    {
        return Some((col, from_abs_y));
    }
    for abs_y in (from_abs_y + 1)..total_lines {
        if let Some(cells) = grid.line_cells(abs_y)
            && let Some(col) = find_cell_match(&cells, query, smart_case, None)
        {
            return Some((col, abs_y));
        }
    }
    for abs_y in 0..from_abs_y {
        if let Some(cells) = grid.line_cells(abs_y)
            && let Some(col) = find_cell_match(&cells, query, smart_case, None)
        {
            return Some((col, abs_y));
        }
    }
    if let Some(cells) = grid.line_cells(from_abs_y)
        && let Some(col) = find_cell_match(&cells, query, smart_case, None)
    {
        return Some((col, from_abs_y));
    }
    None
}

fn find_backward(
    grid: &Grid,
    query: &[char],
    (from_x, from_abs_y): (usize, usize),
    total_lines: usize,
    smart_case: bool,
) -> Option<(usize, usize)> {
    if let Some(cells) = grid.line_cells(from_abs_y)
        && let Some(col) = rfind_cell_match(&cells, query, smart_case, Some(from_x))
    {
        return Some((col, from_abs_y));
    }
    for abs_y in (0..from_abs_y).rev() {
        if let Some(cells) = grid.line_cells(abs_y)
            && let Some(col) = rfind_cell_match(&cells, query, smart_case, None)
        {
            return Some((col, abs_y));
        }
    }
    for abs_y in ((from_abs_y + 1)..total_lines).rev() {
        if let Some(cells) = grid.line_cells(abs_y)
            && let Some(col) = rfind_cell_match(&cells, query, smart_case, None)
        {
            return Some((col, abs_y));
        }
    }
    if let Some(cells) = grid.line_cells(from_abs_y)
        && let Some(col) = rfind_cell_match(&cells, query, smart_case, None)
    {
        return Some((col, from_abs_y));
    }
    None
}

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
    NextWord { big: bool },
    /// `previous-word` and `previous-space`.
    PreviousWord { big: bool },
    /// `next-word-end` and `next-space-end`.
    NextWordEnd { big: bool },
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
    Jump { forward: bool, to: bool, ch: char },
}

/// What a copy command copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyWhat {
    /// The selection; nothing when there is none.
    Selection,
    /// The selection, or the cursor's line when there is none (mpx's vi `y`).
    SelectionOrLine,
    /// The cursor's whole line.
    Line,
    /// From the cursor to the end of its line.
    EndOfLine,
}

/// One copy-mode command, named as tmux's `send-keys -X` names it. The mpx-only names are the
/// ones tmux has no command for: they keep mpx's own key behavior (see `docs/KEYBINDINGS.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyAction {
    /// A move. `cancel_at_bottom` (`cursor-down-and-cancel`) leaves copy mode when the move reaches
    /// the bottom of the live screen.
    Move {
        motion: Motion,
        cancel_at_bottom: bool,
    },
    /// `cancel`: leaves copy mode.
    Cancel,
    /// `clear-selection`.
    ClearSelection,
    /// mpx: clears the selection when there is one, else leaves copy mode (vi `Escape`).
    ClearSelectionOrCancel,
    /// `begin-selection`: a character selection anchored at the cursor.
    BeginSelection,
    /// `select-line`.
    SelectLine,
    /// `select-word`.
    SelectWord,
    /// mpx: starts or clears a character selection (vi `v`, emacs `C-Space`).
    ToggleSelection,
    /// mpx: starts or clears a line selection (vi `V`).
    ToggleLineSelection,
    /// `other-end`: swaps the cursor and the other end of the selection.
    OtherEnd,
    /// `rectangle-toggle` (vi `C-v`): a character selection covers a rectangle instead of a run.
    RectangleToggle,
    /// `copy-*`: copies `what`; `cancel` leaves copy mode, `clear` drops the selection after
    /// copying, and `pipe` also pipes the text to a shell command.
    Copy {
        what: CopyWhat,
        cancel: bool,
        clear: bool,
        pipe: Option<String>,
    },
    /// mpx: copies the selection and leaves copy mode, but only when a selection exists (emacs
    /// `Enter`).
    CopyIfSelected,
    /// `search-forward` / `search-backward` with their text.
    SearchText { direction: SearchDirection, query: String },
    /// `search-again` (`reverse`: `search-reverse`) repeats the last search.
    SearchAgain { reverse: bool },
    /// mpx: opens the search prompt (vi `/` and `?`, emacs `C-s` and `C-r`).
    SearchPrompt(SearchDirection),
}

/// Parses one copy-mode command: `name` and the arguments that follow it. Commands tmux has that
/// mpx cannot express (`rectangle-toggle`, the mark commands, ...) are refused by name.
pub fn parse_copy_command(name: &str, args: &[String]) -> Result<CopyAction, String> {
    let text = || {
        let text = args.join(" ");
        if text.is_empty() {
            Err(format!("send-keys -X {name}: needs an argument"))
        } else {
            Ok(text)
        }
    };
    let pipe = (!args.is_empty()).then(|| args.join(" "));
    let mv = |motion: Motion| CopyAction::Move {
        motion,
        cancel_at_bottom: false,
    };
    let copy = |what: CopyWhat, cancel: bool, clear: bool| CopyAction::Copy {
        what,
        cancel,
        clear,
        pipe: None,
    };
    let piped = |what: CopyWhat, cancel: bool, clear: bool| CopyAction::Copy {
        what,
        cancel,
        clear,
        pipe: pipe.clone(),
    };
    Ok(match name {
        "cursor-left" => mv(Motion::Left),
        "cursor-right" => mv(Motion::Right),
        "cursor-up" => mv(Motion::Up),
        "cursor-down" => mv(Motion::Down),
        "cursor-down-and-cancel" => CopyAction::Move {
            motion: Motion::Down,
            cancel_at_bottom: true,
        },
        "start-of-line" => mv(Motion::LineStart),
        "end-of-line" => mv(Motion::LineEnd),
        "back-to-indentation" => mv(Motion::BackToIndentation),
        "next-word" => mv(Motion::NextWord { big: false }),
        "next-space" => mv(Motion::NextWord { big: true }),
        "previous-word" => mv(Motion::PreviousWord { big: false }),
        "previous-space" => mv(Motion::PreviousWord { big: true }),
        "next-word-end" => mv(Motion::NextWordEnd { big: false }),
        "next-space-end" => mv(Motion::NextWordEnd { big: true }),
        "forward-four-columns" => mv(Motion::FourForward),
        "back-four-columns" => mv(Motion::FourBack),
        "top-line" => mv(Motion::TopLine),
        "middle-line" => mv(Motion::MiddleLine),
        "bottom-line" => mv(Motion::BottomLine),
        "history-top" => mv(Motion::HistoryTop),
        "history-bottom" => mv(Motion::HistoryBottom),
        "page-up" => mv(Motion::PageUp),
        "page-down" => mv(Motion::PageDown),
        "halfpage-up" => mv(Motion::HalfPageUp),
        "halfpage-down" => mv(Motion::HalfPageDown),
        "halfpage-down-and-cancel" => CopyAction::Move {
            motion: Motion::HalfPageDown,
            cancel_at_bottom: true,
        },
        "scroll-up" => mv(Motion::ScrollUp),
        "scroll-down" => mv(Motion::ScrollDown),
        "goto-line" => {
            let raw = args.first().map_or("", String::as_str);
            let line = raw
                .parse::<usize>()
                .map_err(|_| format!("send-keys -X goto-line: invalid line '{raw}'"))?;
            mv(Motion::GotoLine(line))
        }
        "jump-forward" | "jump-backward" | "jump-to-forward" | "jump-to-backward" => {
            let ch = args
                .first()
                .and_then(|a| a.chars().next())
                .ok_or_else(|| format!("send-keys -X {name}: needs a character"))?;
            mv(Motion::Jump {
                forward: name.ends_with("forward"),
                to: name.starts_with("jump-to"),
                ch,
            })
        }
        "cancel" => CopyAction::Cancel,
        "clear-selection" => CopyAction::ClearSelection,
        "clear-selection-or-cancel" => CopyAction::ClearSelectionOrCancel,
        "begin-selection" => CopyAction::BeginSelection,
        "select-line" => CopyAction::SelectLine,
        "select-word" => CopyAction::SelectWord,
        "toggle-selection" => CopyAction::ToggleSelection,
        "toggle-line-selection" => CopyAction::ToggleLineSelection,
        "other-end" => CopyAction::OtherEnd,
        "rectangle-toggle" => CopyAction::RectangleToggle,
        "copy-selection" => copy(CopyWhat::Selection, false, true),
        "copy-selection-no-clear" => copy(CopyWhat::Selection, false, false),
        "copy-selection-and-cancel" => copy(CopyWhat::Selection, true, true),
        "copy-selection-or-line-and-cancel" => copy(CopyWhat::SelectionOrLine, true, false),
        "copy-selection-if-any" => CopyAction::CopyIfSelected,
        "copy-line" => copy(CopyWhat::Line, false, false),
        "copy-line-and-cancel" => copy(CopyWhat::Line, true, false),
        "copy-end-of-line" => copy(CopyWhat::EndOfLine, false, false),
        "copy-end-of-line-and-cancel" => copy(CopyWhat::EndOfLine, true, false),
        "copy-pipe" => piped(CopyWhat::Selection, false, true),
        "copy-pipe-no-clear" => piped(CopyWhat::Selection, false, false),
        "copy-pipe-and-cancel" => piped(CopyWhat::Selection, true, true),
        "copy-pipe-line" => piped(CopyWhat::Line, false, false),
        "copy-pipe-line-and-cancel" => piped(CopyWhat::Line, true, false),
        "copy-pipe-end-of-line" => piped(CopyWhat::EndOfLine, false, false),
        "copy-pipe-end-of-line-and-cancel" => piped(CopyWhat::EndOfLine, true, false),
        "search-forward" => CopyAction::SearchText {
            direction: SearchDirection::Down,
            query: text()?,
        },
        "search-backward" => CopyAction::SearchText {
            direction: SearchDirection::Up,
            query: text()?,
        },
        "search-again" => CopyAction::SearchAgain { reverse: false },
        "search-reverse" => CopyAction::SearchAgain { reverse: true },
        "search-prompt-forward" => CopyAction::SearchPrompt(SearchDirection::Down),
        "search-prompt-backward" => CopyAction::SearchPrompt(SearchDirection::Up),
        _ => return Err(format!("copy-mode command not supported: {name}")),
    })
}

/// Applies one key in copy mode with the default table of `keys` (`mode-keys`). The app applies
/// the key with its own (rebindable) table through `apply_bound_key`.
pub fn apply_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
    keys: crate::config::ModeKeys,
) -> CopyOutcome {
    let action = crate::tui::keymap::default_copy_action(keys, key);
    apply_bound_key(cs, key, action.as_ref(), grid, pane)
}

/// Applies one key in copy mode: the search prompt takes it first when one is open, else `action`
/// (the command the key is bound to) runs. `grid` is the active pane's screen; `pane` its size.
pub fn apply_bound_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    action: Option<&CopyAction>,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    if let Some(search) = cs.search_prompt.take() {
        return handle_search_key(cs, search, key, grid, pane);
    }
    match action {
        Some(action) => apply_action(cs, action, grid, pane),
        None => CopyOutcome::Stay,
    }
}

/// Runs one copy-mode command on `cs`. `grid` is the pane's screen (commands that read text do
/// nothing without it); `pane` its size in cells.
pub fn apply_action(
    cs: &mut CopyState,
    action: &CopyAction,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    match action {
        CopyAction::Move {
            motion,
            cancel_at_bottom,
        } => {
            let before = cs.scroll_offset;
            apply_motion(cs, *motion, grid, pane);
            let downward = matches!(
                motion,
                Motion::Down | Motion::HalfPageDown | Motion::PageDown | Motion::ScrollDown
            );
            let at_bottom = cs.scroll_offset == 0 && cs.cursor_y + 1 >= pane.1 as usize;
            // `copy-mode -e` leaves once the view is back on the live screen.
            if downward && cs.wheel_exit && before > 0 && cs.scroll_offset == 0 {
                return CopyOutcome::Exit;
            }
            if downward && *cancel_at_bottom && at_bottom {
                return CopyOutcome::Exit;
            }
            CopyOutcome::Stay
        }
        CopyAction::Cancel => CopyOutcome::Exit,
        CopyAction::ClearSelection => {
            cs.mode = SelectionKind::None;
            CopyOutcome::Stay
        }
        CopyAction::ClearSelectionOrCancel => {
            if cs.mode == SelectionKind::None {
                return CopyOutcome::Exit;
            }
            cs.mode = SelectionKind::None;
            CopyOutcome::Stay
        }
        CopyAction::BeginSelection => {
            if cs.mode != SelectionKind::Rectangle {
                cs.mode = SelectionKind::Character;
            }
            cs.anchor_x = cs.cursor_x;
            cs.anchor_abs_y = cursor_abs(cs, grid);
            CopyOutcome::Stay
        }
        CopyAction::SelectLine => {
            cs.mode = SelectionKind::Line;
            cs.anchor_x = 0;
            cs.anchor_abs_y = cursor_abs(cs, grid);
            CopyOutcome::Stay
        }
        CopyAction::SelectWord => {
            select_word(cs, grid);
            CopyOutcome::Stay
        }
        CopyAction::ToggleSelection => {
            cs.toggle_char(grid);
            CopyOutcome::Stay
        }
        CopyAction::ToggleLineSelection => {
            cs.toggle_line(grid);
            CopyOutcome::Stay
        }
        CopyAction::RectangleToggle => {
            cs.toggle_rectangle(grid);
            CopyOutcome::Stay
        }
        CopyAction::OtherEnd => {
            other_end(cs, grid, pane);
            CopyOutcome::Stay
        }
        CopyAction::Copy {
            what,
            cancel,
            clear,
            pipe,
        } => copy_outcome(cs, *what, *cancel, *clear, pipe.as_deref(), grid),
        CopyAction::CopyIfSelected => {
            if cs.mode == SelectionKind::None {
                return CopyOutcome::Stay;
            }
            copy_outcome(cs, CopyWhat::Selection, true, false, None, grid)
        }
        CopyAction::SearchText { direction, query } => {
            cs.last_search = Some((*direction, query.clone()));
            execute_search(cs, query, *direction, grid, pane)
        }
        CopyAction::SearchAgain { reverse } => match cs.last_search.clone() {
            Some((dir, query)) => {
                let dir = if *reverse { dir.opposite() } else { dir };
                execute_search(cs, &query, dir, grid, pane)
            }
            None => CopyOutcome::Stay,
        },
        CopyAction::SearchPrompt(direction) => {
            cs.search_prompt = Some(SearchPrompt::new(*direction));
            CopyOutcome::Stay
        }
    }
}

/// The absolute line of the cursor, or its screen row without a grid.
fn cursor_abs(cs: &CopyState, grid: Option<&Grid>) -> usize {
    grid.map_or(cs.cursor_y, |g| cs.cursor_abs_y(g))
}

/// What a copy command hands to the app: the text it copies, and whether it leaves copy mode.
/// `clear` drops the selection; without a text (no selection, or no screen) nothing is copied.
fn copy_outcome(
    cs: &mut CopyState,
    what: CopyWhat,
    cancel: bool,
    clear: bool,
    pipe: Option<&str>,
    grid: Option<&Grid>,
) -> CopyOutcome {
    let text = grid.and_then(|g| match what {
        CopyWhat::Selection => (cs.mode != SelectionKind::None).then(|| cs.yank_text(g)),
        CopyWhat::SelectionOrLine => Some(cs.yank_text(g)),
        CopyWhat::Line => Some(line_text(g, cs.cursor_abs_y(g), 0)),
        CopyWhat::EndOfLine => Some(line_text(g, cs.cursor_abs_y(g), cs.cursor_x)),
    });
    if clear {
        cs.mode = SelectionKind::None;
    }
    match (text, pipe) {
        (Some(text), Some(command)) => CopyOutcome::Pipe {
            text,
            command: command.to_string(),
            exit: cancel,
        },
        (Some(text), None) if cancel => CopyOutcome::Yank(text),
        (Some(text), None) => CopyOutcome::Copy(text),
        (None, _) if cancel => CopyOutcome::Exit,
        (None, _) => CopyOutcome::Stay,
    }
}

/// The text of one history line from column `from`, trailing blanks dropped.
fn line_text(grid: &Grid, abs_y: usize, from: usize) -> String {
    grid.line_cells(abs_y).map_or_else(String::new, |cells| {
        cells
            .iter()
            .skip(from)
            .map(|c| c.c)
            .collect::<String>()
            .trim_end()
            .to_string()
    })
}

/// Selects the run of same-class characters under the cursor, on its line.
fn select_word(cs: &mut CopyState, grid: Option<&Grid>) {
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

/// Swaps the cursor with the other end of the selection.
fn other_end(cs: &mut CopyState, grid: Option<&Grid>, pane: PaneSize) {
    let Some(g) = grid else { return };
    let (cursor_x, cursor_abs) = (cs.cursor_x, cs.cursor_abs_y(g));
    let (anchor_x, anchor_abs) = (cs.anchor_x, cs.anchor_abs_y);
    cs.anchor_x = cursor_x;
    cs.anchor_abs_y = cursor_abs;
    cs.scroll_to_match(anchor_x, anchor_abs, g.history_len(), pane.1 as usize);
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

/// The last character of the next word (vim `e`).
fn next_word_end(w: &Cells, mut p: Pos, big: bool) -> Pos {
    let Some(q) = w.next(p) else { return p };
    p = q;
    while w.class(p, big) == CharClass::Blank {
        let Some(q) = w.next(p) else { return p };
        p = q;
    }
    let class = w.class(p, big);
    while let Some(q) = w.next(p) {
        if w.class(q, big) != class {
            break;
        }
        p = q;
    }
    p
}

/// The start of the previous word (vim `b`).
fn previous_word(w: &Cells, mut p: Pos, big: bool) -> Pos {
    let Some(q) = w.prev(p) else { return p };
    p = q;
    while w.class(p, big) == CharClass::Blank {
        let Some(q) = w.prev(p) else { return p };
        p = q;
    }
    let class = w.class(p, big);
    while let Some(q) = w.prev(p) {
        if w.class(q, big) != class {
            break;
        }
        p = q;
    }
    p
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

fn apply_motion(cs: &mut CopyState, motion: Motion, grid: Option<&Grid>, pane: PaneSize) {
    let (pane_w, pane_h) = (pane.0 as usize, pane.1 as usize);
    let last_col = pane_w.saturating_sub(1);
    let hist_len = grid.map_or(0, Grid::history_len);
    let half = (pane_h / 2).max(1);
    match motion {
        Motion::Left => cs.cursor_x = cs.cursor_x.saturating_sub(1),
        Motion::Right => cs.cursor_x = (cs.cursor_x + 1).min(last_col),
        Motion::Up => {
            if cs.cursor_y > 0 {
                cs.cursor_y -= 1;
            } else if cs.scroll_offset < hist_len {
                cs.scroll_offset += 1;
            }
        }
        Motion::Down => {
            if cs.cursor_y + 1 < pane_h {
                cs.cursor_y += 1;
            } else if cs.scroll_offset > 0 {
                cs.scroll_offset -= 1;
            }
        }
        Motion::LineStart => cs.cursor_x = 0,
        Motion::LineEnd => cs.cursor_x = last_col,
        Motion::BackToIndentation => {
            if let Some(g) = grid {
                let abs_y = cs.cursor_abs_y(g);
                cs.cursor_x = g
                    .line_cells(abs_y)
                    .and_then(|cells| cells.iter().position(|c| !c.c.is_whitespace()))
                    .unwrap_or(0);
            }
        }
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
        Motion::ScrollUp => {
            if cs.scroll_offset < hist_len {
                cs.scroll_offset += 1;
            }
        }
        Motion::ScrollDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(1),
        Motion::HistoryTop => {
            cs.scroll_offset = hist_len;
            cs.cursor_y = 0;
            cs.cursor_x = 0;
        }
        Motion::HistoryBottom => {
            cs.scroll_offset = 0;
            cs.cursor_y = pane_h.saturating_sub(1);
        }
        Motion::GotoLine(line) => {
            if let Some(g) = grid {
                let total = g.history_len() + g.rows;
                let abs_y = line.min(total.saturating_sub(1));
                cs.scroll_to_match(cs.cursor_x, abs_y, hist_len, pane_h);
            }
        }
        Motion::Jump { forward, to, ch } => jump(cs, grid, forward, to, ch),
    }
}
