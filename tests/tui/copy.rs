use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::tui::copy::{
    CopyOutcome, CopyState, SelectionKind, WHEEL_STEP, apply_key, char_selection_text,
};
use mpx::vt::grid::Grid;

fn row(text: &str) -> Option<Vec<char>> {
    Some(text.chars().collect())
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fresh_at_origin() -> CopyState {
    CopyState {
        cursor_x: 0,
        cursor_y: 0,
        scroll_offset: 0,
        anchor_x: 0,
        anchor_abs_y: 0,
        mode: SelectionKind::None,
    }
}

fn grid_with(text: &str) -> Grid {
    let mut grid = Grid::new(10, 3);
    for c in text.chars() {
        grid.put_char(c);
    }
    grid
}

#[test]
fn character_selection_on_one_row_takes_the_span() {
    let lines = vec![row("hello world")];
    assert_eq!(char_selection_text(&lines, (0, 0), (4, 0)), "hello");
    assert_eq!(char_selection_text(&lines, (6, 0), (10, 0)), "world");
}

#[test]
fn character_selection_across_rows_trims_interior_rows_and_joins_with_newlines() {
    let lines = vec![row("abc  "), row("defgh")];
    assert_eq!(char_selection_text(&lines, (1, 0), (2, 1)), "bc\ndef");
}

#[test]
fn character_selection_is_the_same_whichever_endpoint_is_the_anchor() {
    let lines = vec![row("abc  "), row("defgh")];
    assert_eq!(char_selection_text(&lines, (2, 1), (1, 0)), "bc\ndef");
}

#[test]
fn character_selection_skips_rows_that_are_missing() {
    let lines = vec![row("one"), None, row("three")];
    assert_eq!(char_selection_text(&lines, (0, 0), (4, 2)), "one\nthree");
}

#[test]
fn visual_selection_then_y_yanks_the_selected_text() {
    let grid = grid_with("hello");
    let mut cs = fresh_at_origin();
    assert_eq!(
        apply_key(&mut cs, &key('v'), Some(&grid), (10, 3)),
        CopyOutcome::Stay
    );
    for _ in 0..4 {
        apply_key(&mut cs, &code(KeyCode::Right), Some(&grid), (10, 3));
    }
    assert_eq!(
        apply_key(&mut cs, &key('y'), Some(&grid), (10, 3)),
        CopyOutcome::Yank("hello".to_string())
    );
}

#[test]
fn escape_clears_selection_before_leaving_copy_mode() {
    let grid = grid_with("hi");
    let mut cs = fresh_at_origin();
    apply_key(&mut cs, &key('v'), Some(&grid), (10, 3));
    assert_eq!(cs.mode, SelectionKind::Character);
    assert_eq!(
        apply_key(&mut cs, &code(KeyCode::Esc), Some(&grid), (10, 3)),
        CopyOutcome::Stay
    );
    assert_eq!(cs.mode, SelectionKind::None);
    assert_eq!(
        apply_key(&mut cs, &code(KeyCode::Esc), Some(&grid), (10, 3)),
        CopyOutcome::Exit
    );
}

#[test]
fn v_twice_turns_the_selection_off_and_q_exits() {
    let grid = grid_with("x");
    let mut cs = fresh_at_origin();
    apply_key(&mut cs, &key('v'), Some(&grid), (10, 3));
    apply_key(&mut cs, &key('v'), Some(&grid), (10, 3));
    assert_eq!(cs.mode, SelectionKind::None);
    assert_eq!(
        apply_key(&mut cs, &key('q'), Some(&grid), (10, 3)),
        CopyOutcome::Exit
    );
}

#[test]
fn shift_v_selects_whole_lines_and_yank_returns_them() {
    let grid = grid_with("ab");
    let mut cs = fresh_at_origin();
    apply_key(&mut cs, &key('V'), Some(&grid), (10, 3));
    assert_eq!(cs.mode, SelectionKind::Line);
    match apply_key(&mut cs, &key('y'), Some(&grid), (10, 3)) {
        CopyOutcome::Yank(text) => assert_eq!(text, "ab"),
        other => panic!("expected yank, got {other:?}"),
    }
}

#[test]
fn yank_without_a_screen_just_exits() {
    let mut cs = fresh_at_origin();
    assert_eq!(
        apply_key(&mut cs, &key('y'), None, (10, 3)),
        CopyOutcome::Exit
    );
}

#[test]
fn wheel_up_scrolls_into_history_then_moves_the_cursor() {
    let mut cs = fresh_at_origin();
    cs.cursor_y = 2;
    cs.wheel_up(4);
    assert_eq!(cs.scroll_offset, WHEEL_STEP);
    cs.wheel_up(4);
    assert_eq!(
        cs.scroll_offset, 4,
        "scroll is capped at the history length"
    );
    cs.wheel_up(4);
    assert_eq!(cs.scroll_offset, 4);
    assert_eq!(
        cs.cursor_y, 0,
        "cursor moves once history is exhausted, saturating at the top"
    );
}

#[test]
fn wheel_up_without_history_moves_the_cursor_only() {
    let mut cs = fresh_at_origin();
    cs.cursor_y = 5;
    cs.wheel_up(0);
    assert_eq!(cs.scroll_offset, 0);
    assert_eq!(cs.cursor_y, 5 - WHEEL_STEP);
}

#[test]
fn wheel_down_returns_to_live_screen_then_moves_cursor_down_within_the_pane() {
    let mut cs = fresh_at_origin();
    cs.scroll_offset = 4;
    cs.wheel_down(10);
    assert_eq!(cs.scroll_offset, 1);
    cs.wheel_down(10);
    assert_eq!(cs.scroll_offset, 0);
    cs.wheel_down(10);
    assert_eq!(cs.cursor_y, WHEEL_STEP);
    cs.cursor_y = 8;
    cs.wheel_down(10);
    assert_eq!(cs.cursor_y, 9, "cursor stops on the last pane row");
}

#[test]
fn entering_by_wheel_scrolls_only_when_there_is_history() {
    let mut grid = Grid::new(10, 5);
    grid.newline();
    grid.newline();
    let no_history = CopyState::from_wheel(Some(&grid), 0);
    assert_eq!(no_history.scroll_offset, 0);
    assert_eq!(
        no_history.cursor_y, 0,
        "cursor row 2 moves up by a wheel step, saturating"
    );
    let with_history = CopyState::from_wheel(Some(&grid), 5);
    assert_eq!(with_history.scroll_offset, WHEEL_STEP);
    assert_eq!(with_history.cursor_y, 2);
    assert_eq!(with_history.mode, SelectionKind::None);
}
