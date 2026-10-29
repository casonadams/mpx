use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::tui::copy::{
    CopyOutcome, CopyState, SearchDirection, SelectionKind, WHEEL_STEP, apply_key,
    char_selection_text, find_next_match, line_matches,
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
        search_prompt: None,
        last_search: None,
        wheel_exit: false,
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
fn wheel_down_scrolls_back_and_says_when_the_live_screen_shows() {
    let mut cs = fresh_at_origin();
    cs.scroll_offset = 4;
    assert!(!cs.wheel_down());
    assert_eq!(cs.scroll_offset, 1);
    assert!(cs.wheel_down());
    assert_eq!(cs.scroll_offset, 0);
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
#[test]
fn find_next_match_finds_forward_and_backward_across_lines() {
    let mut grid = Grid::new(20, 3);
    for c in "foo first".chars() {
        grid.put_char(c);
    }
    grid.carriage_return();
    grid.newline();
    for c in "second foo".chars() {
        grid.put_char(c);
    }
    let down = find_next_match(&grid, "foo", (0, 0), SearchDirection::Down);
    assert_eq!(down, Some((7, 1)), "finds next foo on row 1");

    let up = find_next_match(&grid, "foo", (7, 1), SearchDirection::Up);
    assert_eq!(up, Some((0, 0)), "finds foo on row 0");
}

#[test]
fn find_next_match_same_line_and_smart_case() {
    let mut grid = Grid::new(30, 2);
    for c in "foo and foo and Foo".chars() {
        grid.put_char(c);
    }
    // Search forward from column 0: finds second foo at 8
    let next1 = find_next_match(&grid, "foo", (0, 0), SearchDirection::Down);
    assert_eq!(next1, Some((8, 0)));
    // Search forward from column 8: finds third Foo at 16 (smart case matches uppercase)
    let next2 = find_next_match(&grid, "foo", (8, 0), SearchDirection::Down);
    assert_eq!(next2, Some((16, 0)));
    // Exact case search for "Foo" does not match lowercase
    let exact = find_next_match(&grid, "Foo", (0, 0), SearchDirection::Down);
    assert_eq!(exact, Some((16, 0)));
    // Backward search from 16 finds previous foo at 8
    let prev = find_next_match(&grid, "foo", (16, 0), SearchDirection::Up);
    assert_eq!(prev, Some((8, 0)));
}


#[test]
fn line_matches_supports_smart_case() {
    assert_eq!(line_matches("Error: syntax error", "error", true), vec![0, 14]);
    assert_eq!(line_matches("Error: syntax error", "Error", false), vec![0]);
    assert_eq!(line_matches("no match here", "test", true), Vec::<usize>::new());
}

#[test]
fn search_down_finds_forward_match_and_positions_cursor() {
    let mut grid = Grid::new(25, 5);
    for c in "alpha beta gamma".chars() {
        grid.put_char(c);
    }
    grid.carriage_return();
    grid.newline();
    for c in "delta beta epsilon".chars() {
        grid.put_char(c);
    }
    let mut cs = fresh_at_origin();
    // Press '/' to start search down
    assert_eq!(apply_key(&mut cs, &key('/'), Some(&grid), (25, 5)), CopyOutcome::Stay);
    assert!(cs.search_prompt.is_some());
    for c in "beta".chars() {
        apply_key(&mut cs, &key(c), Some(&grid), (20, 5));
    }
    // Press Enter to submit
    assert_eq!(apply_key(&mut cs, &code(KeyCode::Enter), Some(&grid), (20, 5)), CopyOutcome::Stay);
    assert!(cs.search_prompt.is_none());
    assert_eq!(cs.cursor_y, 0);
    assert_eq!(cs.cursor_x, 6, "first 'beta' starts at column 6");

    // Press 'n' to find next
    assert_eq!(apply_key(&mut cs, &key('n'), Some(&grid), (25, 5)), CopyOutcome::Stay);
    assert_eq!(cs.cursor_y, 1);
    assert_eq!(cs.cursor_x, 6, "second 'beta' starts on row 1 at column 6");

    // Press 'N' to search in reverse (up)
    assert_eq!(apply_key(&mut cs, &key('N'), Some(&grid), (25, 5)), CopyOutcome::Stay);
    assert_eq!(cs.cursor_y, 0);
    assert_eq!(cs.cursor_x, 6, "returns to first 'beta'");
}

#[test]
fn search_up_finds_backward_match_in_scrollback() {
    let mut grid = Grid::new(20, 3);
    for c in "target_in_history".chars() {
        grid.put_char(c);
    }
    grid.newline(); // pushes to history
    grid.newline(); // pushes to history
    grid.newline(); // pushes to history
    for c in "bottom prompt".chars() {
        grid.put_char(c);
    }
    let mut cs = CopyState::enter(Some(&grid));
    assert_eq!(apply_key(&mut cs, &key('?'), Some(&grid), (20, 3)), CopyOutcome::Stay);
    for c in "target".chars() {
        apply_key(&mut cs, &key(c), Some(&grid), (20, 3));
    }
    assert_eq!(apply_key(&mut cs, &code(KeyCode::Enter), Some(&grid), (20, 3)), CopyOutcome::Stay);
    assert_eq!(cs.cursor_x, 0);
    assert!(cs.scroll_offset > 0, "view scrolled back into history");
    assert_eq!(cs.cursor_abs_y(&grid), 0, "match is on history line 0");
}

#[test]
fn search_cancel_with_escape_leaves_position_intact() {
    let grid = grid_with("hello world");
    let mut cs = fresh_at_origin();
    cs.cursor_x = 3;
    apply_key(&mut cs, &key('/'), Some(&grid), (10, 3));
    apply_key(&mut cs, &key('w'), Some(&grid), (10, 3));
    apply_key(&mut cs, &code(KeyCode::Esc), Some(&grid), (10, 3));
    assert!(cs.search_prompt.is_none());
    assert_eq!(cs.cursor_x, 3, "cursor did not jump on cancel");
}

#[test]
fn search_failed_returns_search_failed_outcome() {
    let grid = grid_with("hello world");
    let mut cs = fresh_at_origin();
    apply_key(&mut cs, &key('/'), Some(&grid), (10, 3));
    for c in "nonexistent".chars() {
        apply_key(&mut cs, &key(c), Some(&grid), (10, 3));
    }
    assert_eq!(
        apply_key(&mut cs, &code(KeyCode::Enter), Some(&grid), (10, 3)),
        CopyOutcome::SearchFailed("nonexistent".to_string())
    );
}

#[test]
fn empty_search_reuses_last_search_pattern() {
    let mut grid = Grid::new(20, 3);
    for c in "abc two one".chars() {
        grid.put_char(c);
    }
    let mut cs = fresh_at_origin();
    apply_key(&mut cs, &key('/'), Some(&grid), (20, 3));
    for c in "one".chars() {
        apply_key(&mut cs, &key(c), Some(&grid), (20, 3));
    }
    apply_key(&mut cs, &code(KeyCode::Enter), Some(&grid), (20, 3));
    assert_eq!(cs.cursor_x, 8);

    // Move cursor back to start and search with empty input
    cs.cursor_x = 0;
    apply_key(&mut cs, &key('/'), Some(&grid), (20, 3));
    apply_key(&mut cs, &code(KeyCode::Enter), Some(&grid), (20, 3));
    assert_eq!(cs.cursor_x, 8, "reused 'one' and jumped to next match");
}
