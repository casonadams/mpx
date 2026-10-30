use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::config::ModeKeys;
use mpx::tui::copy::{CopyOutcome, CopyState, SearchDirection, SelectionKind, apply_key};
use mpx::vt::grid::Grid;

const PANE: (u16, u16) = (10, 4);

fn emacs(cs: &mut CopyState, key: KeyEvent, grid: Option<&Grid>) -> CopyOutcome {
    apply_key(cs, &key, grid, PANE, ModeKeys::Emacs)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn at(x: usize, y: usize) -> CopyState {
    let mut cs = CopyState::enter(None);
    cs.cursor_x = x;
    cs.cursor_y = y;
    cs
}

fn grid_with(text: &str) -> Grid {
    let mut grid = Grid::new(10, 3);
    for c in text.chars() {
        grid.put_char(c);
    }
    grid
}

#[test]
fn ctrl_letters_move_by_character_and_by_line() {
    let mut cs = at(5, 2);
    emacs(&mut cs, ctrl('b'), None);
    assert_eq!(cs.cursor_x, 4);
    emacs(&mut cs, ctrl('f'), None);
    assert_eq!(cs.cursor_x, 5);
    emacs(&mut cs, ctrl('p'), None);
    assert_eq!(cs.cursor_y, 1);
    emacs(&mut cs, ctrl('n'), None);
    assert_eq!(cs.cursor_y, 2);
    emacs(&mut cs, ctrl('a'), None);
    assert_eq!(cs.cursor_x, 0);
    emacs(&mut cs, ctrl('e'), None);
    assert_eq!(cs.cursor_x, PANE.0 as usize - 1);
}

#[test]
fn meta_letters_move_by_word() {
    let mut cs = at(0, 0);
    emacs(&mut cs, alt('f'), None);
    assert_eq!(cs.cursor_x, 4);
    emacs(&mut cs, alt('b'), None);
    assert_eq!(cs.cursor_x, 0);
}

#[test]
fn vi_motions_are_not_bound_in_the_emacs_table() {
    let mut cs = at(0, 0);
    let out = emacs(&mut cs, plain(KeyCode::Char('j')), None);
    assert_eq!(out, CopyOutcome::Stay);
    assert_eq!(cs.cursor_y, 0);
}

#[test]
fn meta_angle_brackets_go_to_the_bottom_and_top() {
    let mut cs = at(3, 1);
    emacs(&mut cs, alt('>'), None);
    assert_eq!((cs.cursor_x, cs.cursor_y), (3, PANE.1 as usize - 1));
    assert_eq!(cs.scroll_offset, 0);
    emacs(&mut cs, alt('<'), None);
    assert_eq!((cs.cursor_x, cs.cursor_y), (0, 0));
}

#[test]
fn ctrl_v_and_meta_v_page_through_history() {
    let mut grid = Grid::new(10, 3);
    for _ in 0..12 {
        grid.put_char('x');
        grid.newline();
    }
    assert!(grid.history_len() > 0, "the grid must have scrollback");
    let mut cs = at(0, 0);
    emacs(&mut cs, alt('v'), Some(&grid));
    assert!(cs.scroll_offset > 0, "M-v scrolls back into history");
    emacs(&mut cs, ctrl('v'), Some(&grid));
    assert_eq!(cs.scroll_offset, 0, "C-v scrolls forward again");
}

#[test]
fn ctrl_space_selects_and_meta_w_copies_then_leaves() {
    let grid = grid_with("hello");
    let mut cs = at(0, 0);
    emacs(&mut cs, ctrl(' '), Some(&grid));
    assert_eq!(cs.mode, SelectionKind::Character);
    emacs(&mut cs, ctrl('f'), Some(&grid));
    match emacs(&mut cs, alt('w'), Some(&grid)) {
        CopyOutcome::Yank(text) => assert!(text.starts_with("he"), "got {text:?}"),
        other => panic!("expected a yank, got {other:?}"),
    }
}

#[test]
fn ctrl_w_without_a_selection_only_leaves_copy_mode() {
    let grid = grid_with("hello");
    let mut cs = at(0, 0);
    assert_eq!(emacs(&mut cs, ctrl('w'), Some(&grid)), CopyOutcome::Exit);
}

#[test]
fn enter_copies_only_when_a_selection_exists() {
    let grid = grid_with("hello");
    let mut cs = at(0, 0);
    assert_eq!(
        emacs(&mut cs, plain(KeyCode::Enter), Some(&grid)),
        CopyOutcome::Stay
    );
    emacs(&mut cs, ctrl(' '), Some(&grid));
    assert!(matches!(
        emacs(&mut cs, plain(KeyCode::Enter), Some(&grid)),
        CopyOutcome::Yank(_)
    ));
}

#[test]
fn ctrl_g_escape_and_q_leave_copy_mode() {
    for key in [ctrl('g'), plain(KeyCode::Esc), plain(KeyCode::Char('q'))] {
        let mut cs = at(0, 0);
        assert_eq!(emacs(&mut cs, key, None), CopyOutcome::Exit);
    }
}

#[test]
fn ctrl_s_and_ctrl_r_open_a_search_in_their_direction() {
    let mut cs = at(0, 0);
    emacs(&mut cs, ctrl('s'), None);
    assert!(matches!(
        cs.search_prompt.as_ref().map(|p| p.direction),
        Some(SearchDirection::Down)
    ));
    let mut cs = at(0, 0);
    emacs(&mut cs, ctrl('r'), None);
    assert!(matches!(
        cs.search_prompt.as_ref().map(|p| p.direction),
        Some(SearchDirection::Up)
    ));
}
