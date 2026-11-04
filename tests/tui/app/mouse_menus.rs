//! Right-click menus and middle-click paste, driven through the harness mouse events.
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::buffer::strip_ansi;
use mpx::config::Config;
use mpx::layout::Direction;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, SIZE, active_window, split, wait_for, wait_output};

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

/// Clicks `button` at (`x`, `y`), draws, and asserts a menu opened showing every entry in `items`.
async fn click_opens_menu(
    h: &mut Harness,
    button: MouseEventKind,
    x: u16,
    y: u16,
    items: &[&str],
) {
    h.mouse(button, x, y).await;
    let drawn = h.frame().await;
    assert!(matches!(h.app.mode, Mode::Menu(_)), "{drawn}");
    for item in items {
        assert!(drawn.contains(*item), "{drawn}");
    }
}

/// A right click on a pane focuses it and opens the pane menu; on a split border it opens the
/// layout menu and focuses nothing.
#[tokio::test]
async fn right_click_opens_the_menu_for_the_pane_or_border_under_the_pointer() {
    // (column offset from the `plain` pane's left edge, active pane afterwards, items shown)
    let cases: [(i16, &str, &[&str]); 2] = [
        (1, "plain", &["Horizontal Split", "Disable Input"]),
        (-1, MAIN, &["Even Horizontal", "Spread Out"]),
    ];
    for (dx, active, items) in cases {
        let mut h = two_panes(Config::default()).await;
        let plain = h.rect_of("plain");
        let x = plain.x.wrapping_add_signed(dx);
        click_opens_menu(&mut h, RIGHT, x, plain.y + 1, items).await;
        assert_eq!(h.app.active_pane, active, "a click at offset {dx} focuses {active}");
    }
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
    // ConPTY puts escape sequences between echoed characters too (`o ESC[?25l k`), so the text is
    // matched with them stripped, not as a contiguous run of the raw bytes.
    let logs = wait_output(&mut h.probe, "plain", |data| {
        strip_ansi(data.as_bytes()).contains("ok")
    })
    .await;
    assert!(!logs.contains("zz"), "logs: {logs}");
}

#[tokio::test]
async fn right_click_on_a_status_tab_selects_its_window_and_opens_the_window_menu() {
    let mut h = two_panes(Config::default()).await;
    h.open_window("work").await;
    let drawn = h.frame().await;
    assert_eq!(active_window(&h), "work");
    let col = status_column(&drawn, "main");
    click_opens_menu(&mut h, RIGHT, col + 1, SIZE.1 - 1, &["Rename", "Kill"]).await;
    assert_eq!(active_window(&h), "main");
}

#[tokio::test]
async fn right_click_on_the_session_name_opens_the_session_menu() {
    let mut h = two_panes(Config::default()).await;
    click_opens_menu(&mut h, RIGHT, 1, SIZE.1 - 1, &["Detach", "Next Session"]).await;
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
