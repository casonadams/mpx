//! Right-click menus and middle-click paste, driven through the harness mouse events.
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::config::Config;
use mpx::layout::Direction;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, SIZE, active_window, split, wait_for};

const RIGHT: MouseEventKind = MouseEventKind::Down(MouseButton::Right);
const MIDDLE: MouseEventKind = MouseEventKind::Down(MouseButton::Middle);

/// A harness with a `plain` pane split to the right of the main pane, drawn once.
async fn two_panes(config: Config) -> Harness {
    let mut h = Harness::start(config, ECHO).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    h
}

/// Column of the first `needle` in the status row, as drawn.
fn status_column(drawn: &str, needle: &str) -> u16 {
    let row = drawn.lines().nth(SIZE.1 as usize - 1).unwrap_or_default();
    row.find(needle).expect("status row shows the tab") as u16
}

#[tokio::test]
async fn right_click_in_a_pane_focuses_it_and_opens_the_pane_menu() {
    let mut h = two_panes(Config::default()).await;
    let plain = h.rect_of("plain");
    h.mouse(RIGHT, plain.x + 1, plain.y + 1).await;
    let drawn = h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(matches!(h.app.mode, Mode::Menu(_)), "{drawn}");
    assert!(drawn.contains("Horizontal Split"), "{drawn}");
    assert!(drawn.contains("Disable Input"), "{drawn}");
}

#[tokio::test]
async fn pane_menu_items_run_their_commands() {
    let mut h = two_panes(Config::default()).await;
    let plain = h.rect_of("plain");
    h.mouse(RIGHT, plain.x + 1, plain.y + 1).await;
    h.frame().await;
    // "Disable Input" is the menu's `i` item: keys typed into the pane are dropped from now on.
    h.key(KeyCode::Char('i')).await;
    assert!(matches!(h.app.mode, Mode::Normal), "the item closes the menu");
    h.type_str("zz").await;
    h.key(KeyCode::Enter).await;
    h.command("select-pane -e").await;
    h.type_str("ok").await;
    h.key(KeyCode::Enter).await;
    let logs = wait_for(&mut h.probe, "plain", "ok\r").await;
    assert!(!logs.contains("zz"), "logs: {logs}");
}

#[tokio::test]
async fn right_click_on_a_status_tab_selects_its_window_and_opens_the_window_menu() {
    let mut h = two_panes(Config::default()).await;
    h.open_window("work").await;
    let drawn = h.frame().await;
    assert_eq!(active_window(&h), "work");
    let col = status_column(&drawn, "main");
    h.mouse(RIGHT, col + 1, SIZE.1 - 1).await;
    let drawn = h.frame().await;
    assert_eq!(active_window(&h), "main");
    assert!(matches!(h.app.mode, Mode::Menu(_)), "{drawn}");
    assert!(drawn.contains("Rename"), "{drawn}");
    assert!(drawn.contains("Kill"), "{drawn}");
}

#[tokio::test]
async fn right_click_on_the_session_name_opens_the_session_menu() {
    let mut h = two_panes(Config::default()).await;
    h.mouse(RIGHT, 1, SIZE.1 - 1).await;
    let drawn = h.frame().await;
    assert!(matches!(h.app.mode, Mode::Menu(_)), "{drawn}");
    assert!(drawn.contains("Detach"), "{drawn}");
    assert!(drawn.contains("Next Session"), "{drawn}");
}

#[tokio::test]
async fn right_click_on_a_split_border_opens_the_layout_menu() {
    let mut h = two_panes(Config::default()).await;
    let plain = h.rect_of("plain");
    h.mouse(RIGHT, plain.x - 1, plain.y + 1).await;
    let drawn = h.frame().await;
    assert_eq!(h.app.active_pane, MAIN, "a border click focuses nothing");
    assert!(matches!(h.app.mode, Mode::Menu(_)), "{drawn}");
    assert!(drawn.contains("Even Horizontal"), "{drawn}");
    assert!(drawn.contains("Spread Out"), "{drawn}");
}

#[tokio::test]
async fn right_click_does_nothing_with_the_mouse_option_off() {
    let mut h = two_panes(Config {
        mouse: false,
        ..Config::default()
    })
    .await;
    let plain = h.rect_of("plain");
    h.mouse(RIGHT, plain.x + 1, plain.y + 1).await;
    h.frame().await;
    assert!(matches!(h.app.mode, Mode::Normal));
    assert_eq!(h.app.active_pane, MAIN);
}

#[tokio::test]
async fn middle_click_pastes_the_top_buffer_into_the_pane() {
    let mut h = two_panes(Config::default()).await;
    h.command("set-buffer pasted-text").await;
    let plain = h.rect_of("plain");
    h.mouse(MIDDLE, plain.x + 1, plain.y + 1).await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(matches!(h.app.mode, Mode::Normal), "middle click opens no menu");
    let logs = wait_for(&mut h.probe, "plain", "pasted-text").await;
    assert!(logs.contains("pasted-text"), "logs: {logs}");
}
