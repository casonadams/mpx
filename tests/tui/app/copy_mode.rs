//! Copy mode (prefix `[`, wheel, mouse drag) and mouse focus, through real input events.
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::config::Config;
use mpx::layout::Direction;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, split};

/// Prints `hello world` on the first row, then READY on the second.
fn two_lines() -> String {
    format!(r"printf 'hello world\r\n'; {ECHO}")
}

/// Prints 40 numbered lines, so the pane has scrollback.
fn long_output() -> String {
    format!(r"for i in $(seq 1 40); do printf 'line %s\r\n' $i; done; {ECHO}")
}

async fn started(script: &str) -> Harness {
    let mut h = Harness::start(Config::default(), script).await;
    h.frame().await;
    h.until("pane text drawn", |app| {
        app.active_vt()
            .is_some_and(|vt| vt.snapshot().contains("READY"))
    })
    .await;
    h
}

#[tokio::test]
async fn prefix_bracket_enters_copy_mode_and_q_leaves_it() {
    let mut h = started(ECHO).await;
    h.prefixed_char('[').await;
    assert!(h.frame().await.contains("COPY -0/0"));
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
    assert!(!h.frame().await.contains("COPY"));
}

#[tokio::test]
async fn keyboard_selection_yanks_the_selected_text() {
    let mut h = started(&two_lines()).await;
    h.prefixed_char('[').await;
    h.key(KeyCode::Char('0')).await;
    h.key(KeyCode::Char('v')).await;
    for _ in 0..4 {
        h.key(KeyCode::Char('l')).await;
    }
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.clipboard(), ["READY"]);
}

#[tokio::test]
async fn mouse_drag_selects_and_release_copies() {
    let mut h = started(&two_lines()).await;
    h.frame().await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 0).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0).await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), 4, 0).await;
    assert_eq!(h.clipboard(), ["hello"]);
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "selection stays on screen"
    );
}

#[tokio::test]
async fn mouse_wheel_scrolls_into_history() {
    let mut h = started(&long_output()).await;
    h.until("scrollback filled", |app| {
        app.active_vt()
            .is_some_and(|vt| vt.grid().history_len() > 0)
    })
    .await;
    h.frame().await;

    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(h.frame().await.contains("COPY -3/"));
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(h.frame().await.contains("COPY -6/"));
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(h.frame().await.contains("COPY -3/"));
}

#[tokio::test]
async fn mouse_click_focuses_the_clicked_pane_without_copy_mode() {
    let mut h = started(ECHO).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    let target = h.rect_of("plain");
    assert_eq!(h.app.active_pane, MAIN);

    let (col, row) = (target.x + 1, target.y + 1);
    h.mouse(MouseEventKind::Down(MouseButton::Left), col, row)
        .await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), col, row)
        .await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(h.mode_is_normal(), "a click only focuses");
    assert!(h.clipboard().is_empty());
}

#[tokio::test]
async fn clicking_another_pane_leaves_copy_mode() {
    let mut h = started(ECHO).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    h.prefixed_char('[').await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));

    let target = h.rect_of("plain");
    h.mouse(MouseEventKind::Down(MouseButton::Left), target.x, target.y)
        .await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn copy_mode_prefix_opens_the_prefix_menu() {
    let mut h = started(ECHO).await;
    h.app.config.which_key_delay_ms = 0;
    h.prefixed_char('[').await;
    h.ctrl('b').await;
    assert!(h.frame().await.contains("new window"));
}

#[tokio::test]
async fn copy_mode_search_up_and_down_moves_cursor_and_scrolls() {
    let mut h = started(&long_output()).await;
    h.until("scrollback filled", |app| {
        app.active_vt()
            .is_some_and(|vt| vt.grid().history_len() > 0)
    })
    .await;
    h.frame().await;
    h.prefixed_char('[').await;
    assert!(h.frame().await.contains("COPY -0/"));

    // Press '?' to start search up
    h.key(KeyCode::Char('?')).await;
    let prompt_frame = h.frame().await;
    assert!(prompt_frame.contains('?'), "shows ? prompt on status row");

    // Type "line 5" and press Enter
    for c in "line 5".chars() {
        h.key(KeyCode::Char(c)).await;
    }
    h.key(KeyCode::Enter).await;

    let match_frame = h.frame().await;
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "still in copy mode after search"
    );
    if let Mode::Copy(cs) = &h.app.mode {
        assert!(cs.scroll_offset > 0, "scrolled back to match: {match_frame}");
        assert_eq!(cs.cursor_x, 0, "cursor at start of 'line 5'");
    }

    // Press 'n' to find next match
    h.key(KeyCode::Char('n')).await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));

    // Press 'q' to leave copy mode
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
}
