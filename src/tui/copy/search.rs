//! Search: the keys of the open search prompt, running a search, and finding matches.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::prompt::PromptState;
use crate::vt::grid::{Cell, Grid};

use super::{CopyOutcome, CopyState, PaneSize, SearchDirection, SearchPrompt};

pub(super) fn handle_search_key(
    cs: &mut CopyState,
    search: SearchPrompt,
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
        (KeyCode::Enter, _) => submit_search(cs, search, grid, pane),
        (KeyCode::Char('u'), true) => edit_search(cs, search, |p| p.clear()),
        (KeyCode::Char('w'), true) => edit_search(cs, search, |p| p.delete_word()),
        (KeyCode::Char(c), false) => edit_search(cs, search, |p| p.insert(c)),
        (KeyCode::Backspace, _) => edit_search(cs, search, |p| p.backspace()),
        (KeyCode::Delete, _) => edit_search(cs, search, |p| p.delete()),
        (KeyCode::Left, _) => edit_search(cs, search, |p| p.left()),
        (KeyCode::Right, _) => edit_search(cs, search, |p| p.right()),
        (KeyCode::Home, _) => edit_search(cs, search, |p| p.home()),
        (KeyCode::End, _) => edit_search(cs, search, |p| p.end()),
        _ => edit_search(cs, search, |_| {}),
    }
}

/// Runs the search the prompt holds, or the last search when the prompt is empty.
fn submit_search(
    cs: &mut CopyState,
    search: SearchPrompt,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
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

/// Applies an edit to the open search prompt, which stays open.
fn edit_search(
    cs: &mut CopyState,
    mut search: SearchPrompt,
    edit: impl FnOnce(&mut PromptState),
) -> CopyOutcome {
    edit(&mut search.prompt);
    cs.search_prompt = Some(search);
    CopyOutcome::Stay
}

pub(super) fn execute_search(
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
    if let Some((col, abs_y)) = find_next_match(g, query, from, dir, cs.wrap_search) {
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
    let q_len = query_chars.len();
    if line_chars.len() < q_len {
        return Vec::new();
    }
    (0..=line_chars.len() - q_len)
        .filter(|&i| chars_match(line_chars[i..].iter().copied(), &query_chars, smart_case))
        .collect()
}

pub fn find_next_match(
    grid: &Grid,
    query: &str,
    from: (usize, usize),
    direction: SearchDirection,
    wrap: bool,
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
    let search = Search {
        query: &query_chars,
        smart_case,
        wrap,
        forward: matches!(direction, SearchDirection::Down),
    };
    find_match(grid, search, from, total_lines)
}

/// What a search looks for: the query, its case rule, whether it wraps and its direction.
struct Search<'a> {
    query: &'a [char],
    smart_case: bool,
    wrap: bool,
    forward: bool,
}

/// The first match from the cursor in the search direction. The cursor's own line is searched
/// only past (or before) the cursor first; with `wrap` the search comes round to it again.
fn find_match(
    grid: &Grid,
    search: Search,
    (from_x, from_abs_y): (usize, usize),
    total_lines: usize,
) -> Option<(usize, usize)> {
    let Search {
        query,
        smart_case,
        wrap,
        forward,
    } = search;
    let hit = |abs_y: usize, bound: Option<usize>| {
        let cells = grid.line_cells(abs_y)?;
        let col = find_cell_match(&cells, query, smart_case, bound, forward)?;
        Some((col, abs_y))
    };
    let scan = |lines: std::ops::Range<usize>| {
        if forward {
            lines.into_iter().find_map(|y| hit(y, None))
        } else {
            lines.rev().find_map(|y| hit(y, None))
        }
    };
    let (ahead, behind) = if forward {
        (from_abs_y + 1..total_lines, 0..from_abs_y)
    } else {
        (0..from_abs_y, from_abs_y + 1..total_lines)
    };
    let first = hit(from_abs_y, Some(from_x)).or_else(|| scan(ahead));
    if first.is_some() || !wrap {
        return first;
    }
    scan(behind).or_else(|| hit(from_abs_y, None))
}

/// The column of the first (`forward`) or last match of `query` in one row, looking only at the
/// row's text before its trailing blanks. `bound` is the column a forward search starts past or
/// a backward search stops before; `None` searches the whole row.
fn find_cell_match(
    cells: &[Cell],
    query: &[char],
    smart_case: bool,
    bound: Option<usize>,
    forward: bool,
) -> Option<usize> {
    let len = trimmed_cell_len(cells);
    if len < query.len() {
        return None;
    }
    let last = len - query.len();
    let matches_at = |i: usize| chars_match(cells[i..].iter().map(|c| c.c), query, smart_case);
    if forward {
        let start = bound.map_or(0, |c| c + 1);
        (start..=last).find(|&i| matches_at(i))
    } else {
        let max_i = match bound {
            Some(0) => return None,
            Some(max) => (max - 1).min(last),
            None => last,
        };
        (0..=max_i).rev().find(|&i| matches_at(i))
    }
}

fn chars_match(text: impl Iterator<Item = char>, query: &[char], smart_case: bool) -> bool {
    text.zip(query).all(|(a, &b)| cell_match(a, b, smart_case))
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
