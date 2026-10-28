//! Copy (vi-style visual) mode: cursor, scroll, selection, search and yank.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::prompt::PromptState;
use super::render::SelectionRange;
use crate::vt::grid::Grid;

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
        }
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

    /// One wheel notch down: scroll back toward the live screen, then move the cursor down.
    pub fn wheel_down(&mut self, pane_h: usize) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(WHEEL_STEP);
        } else {
            self.cursor_y = (self.cursor_y + WHEEL_STEP).min(pane_h.saturating_sub(1));
        }
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
                .get_line_cells(cursor_abs_y)
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
        if let Some(cells) = grid.get_line_cells(y) {
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
        .map(|y| grid.get_line_cells(y).map(|cells| cells.iter().map(|c| c.c).collect()))
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
            row_text(cells, from_col, cells.len().saturating_sub(1)).map(|s| s.trim_end().to_string())
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

/// Applies one key in copy mode. `grid` is the active pane's screen; `pane` its size in cells.
pub fn apply_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    if let Some(search) = cs.search_prompt.take() {
        return handle_search_key(cs, search, key, grid, pane);
    }
    let hist_len = grid.map_or(0, |g| g.history_len());
    if move_cursor(cs, key, hist_len, pane) {
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
    let smart_case = query.chars().all(|c| !c.is_uppercase());
    let from = (from.0, from.1.min(total_lines.saturating_sub(1)));
    match direction {
        SearchDirection::Down => find_forward(grid, query, from, total_lines, smart_case),
        SearchDirection::Up => find_backward(grid, query, from, total_lines, smart_case),
    }
}

fn find_forward(
    grid: &Grid,
    query: &str,
    (from_x, from_abs_y): (usize, usize),
    total_lines: usize,
    smart_case: bool,
) -> Option<(usize, usize)> {
    if let Some(text) = grid.line_text(from_abs_y)
        && let Some(&col) = line_matches(&text, query, smart_case)
            .iter()
            .find(|&&c| c > from_x)
    {
        return Some((col, from_abs_y));
    }
    for abs_y in (from_abs_y + 1)..total_lines {
        if let Some(text) = grid.line_text(abs_y)
            && let Some(&col) = line_matches(&text, query, smart_case).first()
        {
            return Some((col, abs_y));
        }
    }
    for abs_y in 0..from_abs_y {
        if let Some(text) = grid.line_text(abs_y)
            && let Some(&col) = line_matches(&text, query, smart_case).first()
        {
            return Some((col, abs_y));
        }
    }
    if let Some(text) = grid.line_text(from_abs_y)
        && let Some(&col) = line_matches(&text, query, smart_case).first()
    {
        return Some((col, from_abs_y));
    }
    None
}

fn find_backward(
    grid: &Grid,
    query: &str,
    (from_x, from_abs_y): (usize, usize),
    total_lines: usize,
    smart_case: bool,
) -> Option<(usize, usize)> {
    if let Some(text) = grid.line_text(from_abs_y)
        && let Some(&col) = line_matches(&text, query, smart_case)
            .iter()
            .rev()
            .find(|&&c| c < from_x)
    {
        return Some((col, from_abs_y));
    }
    for abs_y in (0..from_abs_y).rev() {
        if let Some(text) = grid.line_text(abs_y)
            && let Some(&col) = line_matches(&text, query, smart_case).last()
        {
            return Some((col, abs_y));
        }
    }
    for abs_y in ((from_abs_y + 1)..total_lines).rev() {
        if let Some(text) = grid.line_text(abs_y)
            && let Some(&col) = line_matches(&text, query, smart_case).last()
        {
            return Some((col, abs_y));
        }
    }
    if let Some(text) = grid.line_text(from_abs_y)
        && let Some(&col) = line_matches(&text, query, smart_case).last()
    {
        return Some((col, from_abs_y));
    }
    None
}

fn move_cursor(cs: &mut CopyState, key: &KeyEvent, hist_len: usize, pane: PaneSize) -> bool {
    let (pane_w, pane_h) = (pane.0 as usize, pane.1 as usize);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let last_col = pane_w.saturating_sub(1);
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => cs.cursor_x = cs.cursor_x.saturating_sub(1),
        KeyCode::Char('l') | KeyCode::Right => cs.cursor_x = (cs.cursor_x + 1).min(last_col),
        KeyCode::Char('k') | KeyCode::Up => {
            if cs.cursor_y > 0 {
                cs.cursor_y -= 1;
            } else if cs.scroll_offset < hist_len {
                cs.scroll_offset += 1;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if cs.cursor_y + 1 < pane_h {
                cs.cursor_y += 1;
            } else if cs.scroll_offset > 0 {
                cs.scroll_offset -= 1;
            }
        }
        KeyCode::Char('0') | KeyCode::Char('^') => cs.cursor_x = 0,
        KeyCode::Char('$') => cs.cursor_x = last_col,
        KeyCode::Char('w') => cs.cursor_x = (cs.cursor_x + WORD).min(last_col),
        KeyCode::Char('b') => cs.cursor_x = cs.cursor_x.saturating_sub(WORD),
        KeyCode::Char('u') if ctrl => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        KeyCode::Char('d') if ctrl => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        KeyCode::PageUp => cs.scroll_offset = (cs.scroll_offset + PAGE).min(hist_len),
        KeyCode::PageDown => cs.scroll_offset = cs.scroll_offset.saturating_sub(PAGE),
        KeyCode::Char('g') => {
            cs.scroll_offset = hist_len;
            cs.cursor_y = 0;
            cs.cursor_x = 0;
        }
        KeyCode::Char('G') => {
            cs.scroll_offset = 0;
            cs.cursor_y = pane_h.saturating_sub(1);
        }
        _ => return false,
    }
    true
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
