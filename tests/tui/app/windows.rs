//! Prefix bindings that split, open, rename, kill, move and resize panes and windows.
//! Panes spawn `ECHO` (never the user's shell) through config bindings.
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::tui::app::{Flow, Mode};

use super::support::{ECHO, Harness, MAIN};

/// Default bindings that would spawn the user's shell, rebound to run `ECHO`.
fn echo_config() -> Config {
    let echo = |cmd: &str| format!("{cmd} sh -c '{ECHO}'");
    let mut config = Config::default();
    config.bind.insert("\"".into(), echo("split-window"));
    config.bind.insert("%".into(), echo("split-window -h"));
    config.bind.insert("c".into(), echo("new-window -n work"));
    config
}

async fn start() -> Harness {
    Harness::start(echo_config(), ECHO).await
}

/// Width of `pane` in the last prepared frame.
fn width_of(h: &Harness, pane: &str) -> u16 {
    h.rect_of(pane).width
}

#[tokio::test]
async fn split_and_window_bindings_create_panes_and_windows() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    assert_eq!(h.layout_panes().await, [MAIN, "pane-2"]);
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2", "new split takes focus");

    h.prefixed_char('"').await;
    assert_eq!(h.layout_panes().await.len(), 3);

    h.prefixed_char('c').await;
    assert_eq!(h.windows().await.len(), 2);
    let drawn = h.frame().await;
    assert!(drawn.contains("1:"), "status row lists the new window");
    assert_eq!(h.app.active_pane, "work");
}

#[tokio::test]
async fn rename_window_through_the_command_prompt() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(',').await;
    h.type_str("logs").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await, ["logs"]);
    assert!(h.frame().await.contains("[test] 0:logs*"));
}

#[tokio::test]
async fn kill_pane_asks_first_and_only_y_kills() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('"').await;
    h.frame().await;

    h.prefixed_char('x').await;
    assert!(h.frame().await.contains("kill-pane pane-2? (y/n)"));
    h.key(KeyCode::Char('n')).await;
    assert_eq!(h.layout_panes().await.len(), 2, "n keeps the pane");

    h.prefixed_char('x').await;
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.layout_panes().await, [MAIN]);
}

#[tokio::test]
async fn kill_window_asks_first_and_y_kills_the_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('c').await;
    h.frame().await;
    h.prefixed_char('&').await;
    assert!(h.frame().await.contains("kill-window? (y/n)"));
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.windows().await, [MAIN]);
}

#[tokio::test]
async fn swap_pane_exchanges_neighbours_in_the_layout() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.layout_panes().await, [MAIN, "pane-2"]);

    h.prefixed_char('{').await;
    assert_eq!(h.layout_panes().await, ["pane-2", MAIN]);
}

#[tokio::test]
async fn break_pane_moves_the_pane_into_its_own_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    h.prefixed_char('!').await;
    assert_eq!(h.windows().await.len(), 2);
    assert!(h.nodes().await.contains(&"pane-2".to_string()));
}

#[tokio::test]
async fn zoom_shows_only_the_active_pane_until_toggled_back() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.panes.len(), 2);

    h.prefixed_char('z').await;
    h.frame().await;
    assert_eq!(h.app.zoomed.as_deref(), Some("pane-2"));
    assert_eq!(h.app.panes.len(), 1);

    h.prefixed_char('z').await;
    h.frame().await;
    assert!(h.app.zoomed.is_none());
    assert_eq!(h.app.panes.len(), 2);
}

#[tokio::test]
async fn resize_moves_the_split_and_repeats_inside_the_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    let before = width_of(&h, MAIN);

    h.prefixed_char('H').await;
    h.frame().await;
    let after_prefix = width_of(&h, MAIN);
    assert!(after_prefix < before, "prefix H shrinks the left pane");

    h.key(KeyCode::Char('H')).await;
    h.frame().await;
    assert!(
        width_of(&h, MAIN) < after_prefix,
        "a bare H inside the repeat window resizes again"
    );
}

#[tokio::test]
async fn directional_bindings_move_focus_between_panes() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");

    h.prefixed_char('h').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);

    h.prefixed_char('l').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");
}

#[tokio::test]
async fn detach_binding_quits_the_app() {
    let mut h = start().await;
    h.frame().await;
    assert_eq!(h.prefixed_char('d').await, Flow::Quit);
    assert!(matches!(h.app.mode, Mode::Normal));
}
