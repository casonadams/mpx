//! Keys routed to an open overlay: closing it, and running the commands it hands back.
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN};

#[tokio::test]
async fn tree_overlay_enter_runs_the_selected_window_command() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.open_window("work").await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");

    h.prefixed_char('p').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);

    h.prefixed_char('w').await;
    h.key(KeyCode::Down).await;
    h.closes_with(|mode| matches!(mode, Mode::Overlay(_)), KeyCode::Enter)
        .await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");
}

#[tokio::test]
async fn ctrl_space_palette_lifecycle_and_execution() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.ctrl(' ').await;
    assert!(matches!(h.app.mode, Mode::Overlay(_)));
    assert!(h.frame().await.contains("mpx palette"));
    assert!(h.frame().await.contains("[ Commands ]"));

    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());

    h.ctrl(' ').await;
    h.type_str("vertical").await;
    h.key(KeyCode::Enter).await;
    assert!(h.mode_is_normal());
    h.frame().await;
    assert_eq!(h.app.panes.len(), 2);
}
