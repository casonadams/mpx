//! Running copy-mode commands and keys against a `CopyState`, and reporting what the app must do.
use crossterm::event::KeyEvent;

use crate::config::ModeKeys;
use crate::vt::grid::{Grid, ShellMark};

use super::action::{CopyAction, CopyWhat};
use super::motion::{apply_motion, select_word};
use super::search::{execute_search, handle_search_key};
use super::selection::line_text;
use super::{CopyOutcome, CopyState, Motion, PaneSize, SearchPrompt, SelectionKind};

/// Applies one key in copy mode with the default table of `keys` (`mode-keys`). The app applies
/// the key with its own (rebindable) table through `apply_bound_key`.
pub fn apply_key(
    cs: &mut CopyState,
    key: &KeyEvent,
    grid: Option<&Grid>,
    pane: PaneSize,
    keys: ModeKeys,
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
        } => apply_move(cs, *motion, *cancel_at_bottom, grid, pane),
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
        CopyAction::ToggleLineSelection => {
            cs.toggle_line(grid);
            CopyOutcome::Stay
        }
        CopyAction::ToggleSelection
        | CopyAction::RectangleToggle
        | CopyAction::SetMark
        | CopyAction::JumpToMark
        | CopyAction::SetRectangle(_)
        | CopyAction::JumpToPrompt { .. } => apply_marks(cs, action, grid, pane),
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
        CopyAction::SearchAgain { reverse } => search_again(cs, *reverse, grid, pane),
        CopyAction::SearchPrompt(direction) => {
            cs.search_prompt = Some(SearchPrompt::new(*direction));
            CopyOutcome::Stay
        }
    }
}

/// Runs a cursor or scroll move. `cancel_at_bottom` leaves copy mode once a downward move reaches
/// the bottom; a move back onto the live screen leaves it under `copy-mode -e`.
fn apply_move(
    cs: &mut CopyState,
    motion: Motion,
    cancel_at_bottom: bool,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    let before = cs.scroll_offset;
    apply_motion(cs, motion, grid, pane);
    let downward = matches!(
        motion,
        Motion::Down | Motion::HalfPageDown | Motion::PageDown | Motion::ScrollDown
    );
    let at_bottom = cs.scroll_offset == 0 && cs.cursor_y + 1 >= pane.1 as usize;
    // `copy-mode -e` leaves once the view is back on the live screen.
    if downward && cs.wheel_exit && before > 0 && cs.scroll_offset == 0 {
        return CopyOutcome::Exit;
    }
    if downward && cancel_at_bottom && at_bottom {
        return CopyOutcome::Exit;
    }
    CopyOutcome::Stay
}

/// Repeats the last search, in its own direction or the opposite one when `reverse`.
fn search_again(
    cs: &mut CopyState,
    reverse: bool,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    match cs.last_search.clone() {
        Some((dir, query)) => {
            let dir = if reverse { dir.opposite() } else { dir };
            execute_search(cs, &query, dir, grid, pane)
        }
        None => CopyOutcome::Stay,
    }
}

/// The absolute line of the cursor, or its screen row without a grid.
fn cursor_abs(cs: &CopyState, grid: Option<&Grid>) -> usize {
    grid.map_or(cs.cursor_y, |g| cs.cursor_abs_y(g))
}

/// The mark, rectangle and shell-prompt commands: they set the mark or the selection's shape, or
/// move the cursor to a mark or a prompt.
fn apply_marks(
    cs: &mut CopyState,
    action: &CopyAction,
    grid: Option<&Grid>,
    pane: PaneSize,
) -> CopyOutcome {
    match action {
        CopyAction::ToggleSelection => cs.toggle_char(grid),
        CopyAction::RectangleToggle => cs.toggle_rectangle(grid),
        CopyAction::SetMark => cs.mark = grid.map(|g| g.row_at(cs.cursor_abs_y(g))),
        CopyAction::JumpToMark => jump_to_mark(cs, grid, pane),
        CopyAction::SetRectangle(on) => set_rectangle(cs, *on, grid),
        CopyAction::JumpToPrompt { forward, output } => {
            jump_to_prompt(cs, grid, *forward, *output, pane);
        }
        _ => {}
    }
    CopyOutcome::Stay
}

/// `jump-to-mark`: the cursor moves to the marked line and the mark moves to the line the cursor
/// left, so a second jump comes back. A mark whose row has left history is dropped.
fn jump_to_mark(cs: &mut CopyState, grid: Option<&Grid>, pane: PaneSize) {
    let (Some(g), Some(row)) = (grid, cs.mark) else {
        return;
    };
    let Some(target) = g.abs_at(row) else {
        cs.mark = None;
        return;
    };
    cs.mark = Some(g.row_at(cs.cursor_abs_y(g)));
    cs.scroll_to_abs_y(target, g.history_len(), pane.1 as usize);
}

/// `rectangle-on` and `rectangle-off`: set the selection's shape instead of toggling it.
fn set_rectangle(cs: &mut CopyState, on: bool, grid: Option<&Grid>) {
    if on && cs.mode != SelectionKind::Rectangle {
        cs.toggle_rectangle(grid);
    } else if !on && cs.mode == SelectionKind::Rectangle {
        cs.mode = SelectionKind::Character;
    }
}

/// `next-prompt` and `previous-prompt`: the cursor moves to the next (or previous) shell mark of
/// the kind asked for, and nothing happens when there is none.
fn jump_to_prompt(
    cs: &mut CopyState,
    grid: Option<&Grid>,
    forward: bool,
    output: bool,
    pane: PaneSize,
) {
    let Some(g) = grid else { return };
    let kind = if output {
        ShellMark::Output
    } else {
        ShellMark::Prompt
    };
    let here = cs.cursor_abs_y(g);
    let target = if forward {
        g.shell_marks(kind).find(|&line| line > here)
    } else {
        g.shell_marks(kind).filter(|&line| line < here).last()
    };
    if let Some(line) = target {
        cs.scroll_to_abs_y(line, g.history_len(), pane.1 as usize);
        cs.cursor_x = 0;
    }
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

/// Swaps the cursor with the other end of the selection.
fn other_end(cs: &mut CopyState, grid: Option<&Grid>, pane: PaneSize) {
    let Some(g) = grid else { return };
    let (cursor_x, cursor_abs) = (cs.cursor_x, cs.cursor_abs_y(g));
    let (anchor_x, anchor_abs) = (cs.anchor_x, cs.anchor_abs_y);
    cs.anchor_x = cursor_x;
    cs.anchor_abs_y = cursor_abs;
    cs.scroll_to_match(anchor_x, anchor_abs, g.history_len(), pane.1 as usize);
}
