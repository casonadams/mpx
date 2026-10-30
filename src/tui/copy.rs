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

/// What the key handler asks the app to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    Stay,
    Exit,
    Yank(String),
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

/// Applies one key in copy mode. `grid` is the active pane's screen; `pane` its size in cells; `keys`
/// the key table (`mode-keys`).
pub fn apply_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
    keys: crate::config::ModeKeys,
) -> CopyOutcome {
    if let Some(search) = cs.search_prompt.take() {
        return handle_search_key(cs, search, key, grid, pane);
    }
    let hist_len = grid.map_or(0, |g| g.history_len());
    match keys {
        crate::config::ModeKeys::Vi => vi_key(cs, key, grid, pane, hist_len),
        crate::config::ModeKeys::Emacs => emacs_key(cs, key, grid, pane, hist_len),
    }
}

fn vi_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
    hist_len: usize,
) -> CopyOutcome {
    if let Some(motion) = vi_motion(key) {
        apply_motion(cs, motion, hist_len, pane);
        return CopyOutcome::Stay;
    }
    match key.code {
        KeyCode::Char('?') => {
            cs.search_prompt = Some(SearchPrompt::new(SearchDirection::Up));
            CopyOutcome::Stay
        }
        KeyCode::Char('/') => {
            cs.search_prompt = Some(SearchPrompt::new(SearchDirection::Down));
            CopyOutcome::Stay
        }
        KeyCode::Char('n') => {
            if let Some((dir, query)) = cs.last_search.clone() {
                return execute_search(cs, &query, dir, grid, pane);
            }
            CopyOutcome::Stay
        }
        KeyCode::Char('N') => {
            if let Some((dir, query)) = cs.last_search.clone() {
                return execute_search(cs, &query, dir.opposite(), grid, pane);
            }
            CopyOutcome::Stay
        }
        _ => select_or_exit(cs, key, grid),
    }
}

/// The emacs table (`mode-keys emacs`): C-b/f/p/n, M-b/f, C-a/e, C-v/M-v, M-</M->, C-Space selects,
/// C-w/M-w copy the selection and leave, C-g/Esc/q leave, C-s/C-r search, Enter copies a selection.
fn emacs_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
    hist_len: usize,
) -> CopyOutcome {
    if let Some(motion) = emacs_motion(key) {
        apply_motion(cs, motion, hist_len, pane);
        return CopyOutcome::Stay;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match (key.code, ctrl, alt) {
        (KeyCode::Char(' '), true, _) => cs.toggle_char(grid),
        (KeyCode::Char('w'), true, false) | (KeyCode::Char('w'), false, true) => {
            return copy_selection(cs, grid);
        }
        (KeyCode::Enter, _, _) if cs.mode != SelectionKind::None => {
            return copy_selection(cs, grid);
        }
        (KeyCode::Char('g'), true, false)
        | (KeyCode::Esc, _, _)
        | (KeyCode::Char('q'), false, false) => {
            return CopyOutcome::Exit;
        }
        (KeyCode::Char('s'), true, false) => {
            cs.search_prompt = Some(SearchPrompt::new(SearchDirection::Down));
        }
        (KeyCode::Char('r'), true, false) => {
            cs.search_prompt = Some(SearchPrompt::new(SearchDirection::Up));
        }
        _ => {}
    }
    CopyOutcome::Stay
}

/// Copies the selection and leaves copy mode; with no selection, only leaves.
fn copy_selection(cs: &CopyState, grid: Option<&Grid>) -> CopyOutcome {
    match grid {
        Some(g) if cs.mode != SelectionKind::None => CopyOutcome::Yank(cs.yank_text(g)),
        _ => CopyOutcome::Exit,
    }
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

/// A cursor or scroll move, shared by the vi and emacs tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    WordForward,
    WordBack,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// The vi motion a key stands for.
fn vi_motion(key: &KeyEvent) -> Option<Motion> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('h') | KeyCode::Left => Motion::Left,
        KeyCode::Char('l') | KeyCode::Right => Motion::Right,
        KeyCode::Char('k') | KeyCode::Up => Motion::Up,
        KeyCode::Char('j') | KeyCode::Down => Motion::Down,
        KeyCode::Char('0') | KeyCode::Char('^') => Motion::LineStart,
        KeyCode::Char('$') => Motion::LineEnd,
        KeyCode::Char('w') => Motion::WordForward,
        KeyCode::Char('b') => Motion::WordBack,
        KeyCode::Char('u') if ctrl => Motion::PageUp,
        KeyCode::Char('d') if ctrl => Motion::PageDown,
        KeyCode::PageUp => Motion::PageUp,
        KeyCode::PageDown => Motion::PageDown,
        KeyCode::Char('g') => Motion::Top,
        KeyCode::Char('G') => Motion::Bottom,
        _ => return None,
    })
}

/// The emacs motion a key stands for.
fn emacs_motion(key: &KeyEvent) -> Option<Motion> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    Some(match (key.code, ctrl, alt) {
        (KeyCode::Char('b'), true, false) | (KeyCode::Left, _, _) => Motion::Left,
        (KeyCode::Char('f'), true, false) | (KeyCode::Right, _, _) => Motion::Right,
        (KeyCode::Char('p'), true, false) | (KeyCode::Up, _, _) => Motion::Up,
        (KeyCode::Char('n'), true, false) | (KeyCode::Down, _, _) => Motion::Down,
        (KeyCode::Char('b'), false, true) => Motion::WordBack,
        (KeyCode::Char('f'), false, true) => Motion::WordForward,
        (KeyCode::Char('a'), true, false) | (KeyCode::Home, _, _) => Motion::LineStart,
        (KeyCode::Char('e'), true, false) | (KeyCode::End, _, _) => Motion::LineEnd,
        (KeyCode::Char('v'), true, false) | (KeyCode::PageDown, _, _) => Motion::PageDown,
        (KeyCode::Char('v'), false, true) | (KeyCode::PageUp, _, _) => Motion::PageUp,
        (KeyCode::Char('<'), _, true) => Motion::Top,
        (KeyCode::Char('>'), _, true) => Motion::Bottom,
        _ => return None,
    })
}

fn apply_motion(cs: &mut CopyState, motion: Motion, hist_len: usize, pane: PaneSize) {
    let (pane_w, pane_h) = (pane.0 as usize, pane.1 as usize);
    let last_col = pane_w.saturating_sub(1);
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
        Motion::WordForward => cs.cursor_x = (cs.cursor_x + WORD).min(last_col),
        Motion::WordBack => cs.cursor_x = cs.cursor_x.saturating_sub(WORD),
        Motion::PageUp => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        Motion::PageDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        Motion::Top => {
            cs.scroll_offset = hist_len;
            cs.cursor_y = 0;
            cs.cursor_x = 0;
        }
        Motion::Bottom => {
            cs.scroll_offset = 0;
            cs.cursor_y = pane_h.saturating_sub(1);
        }
    }
}

fn select_or_exit(cs: &mut CopyState, key: &KeyEvent, grid: Option<&Grid>) -> CopyOutcome {
    match key.code {
        KeyCode::Char('V') => cs.toggle_line(grid),
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::SHIFT) => cs.toggle_line(grid),
        KeyCode::Char('v') => cs.toggle_char(grid),
        KeyCode::Char('y') | KeyCode::Enter => {
            return match grid {
                Some(g) => CopyOutcome::Yank(cs.yank_text(g)),
                None => CopyOutcome::Exit,
            };
        }
        KeyCode::Esc => {
            if cs.mode == SelectionKind::None {
                return CopyOutcome::Exit;
            }
            cs.mode = SelectionKind::None;
        }
        KeyCode::Char('q') => return CopyOutcome::Exit,
        _ => {}
    }
    CopyOutcome::Stay
}
