//! `set-option` and `show-options` at the command prompt change the running client.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::config::Config;

use super::support::{ECHO, Harness};

fn ctrl_a() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
}

#[tokio::test]
async fn set_option_changes_the_prefix_and_the_keymap_follows() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.command("set -g prefix C-a").await;
    assert_eq!(h.app.config.prefix, "C-a");
    assert!(h.app.keymap.is_prefix(&ctrl_a()));
    assert!(!h.app.keymap.is_prefix(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)));
}

#[tokio::test]
async fn unset_returns_the_prefix_to_its_default() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.command("set -g prefix C-a ; set -gu prefix").await;
    assert_eq!(h.app.config.prefix, "C-b");
    assert!(!h.app.keymap.is_prefix(&ctrl_a()));
}

#[tokio::test]
async fn an_unresolvable_prefix_is_refused_and_the_old_one_stays() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    let said = h.command_message("set -g prefix not-a-key").await.unwrap_or_default();
    assert!(said.starts_with("set-option:"), "{said}");
    assert_eq!(h.app.config.prefix, "C-b");
}

#[tokio::test]
async fn set_option_reports_unknown_and_daemon_options_and_quiet_hides_unknown() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    let said = h.command_message("set -g nonsense 1").await.unwrap_or_default();
    assert_eq!(said, "set-option: unknown option: nonsense");
    let said = h.command_message("set -g remain-on-exit on").await.unwrap_or_default();
    assert!(said.contains("reload-config"), "{said}");
    h.app.message = None;
    let said = h.command_message("set -gq nonsense 1").await;
    assert_eq!(said, None);
}

#[tokio::test]
async fn show_options_prints_one_value_or_lists_all() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.command("set -g status-position top").await;
    assert_eq!(h.command_message("show -gv status-position").await.as_deref(), Some("top"));
    assert_eq!(
        h.command_message("show-options -g mouse").await.as_deref(),
        Some("mouse on")
    );
    let frame = h.command_frame("show-options").await;
    assert!(frame.contains("status-position top"), "{frame}");
}
