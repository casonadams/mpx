//! `prefix + r` re-reads the config file into the running app.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::config::Config;

use super::support::{ECHO, Harness};

fn ctrl_a() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
}

fn said(h: &Harness) -> String {
    h.app
        .interaction
        .message
        .as_ref()
        .map(|(m, _)| m.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn reload_applies_the_file_and_keeps_the_old_config_when_it_is_invalid() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    h.app.launch.config_path = Some(file.clone());
    assert!(!h.app.keymap.is_prefix(&ctrl_a()));

    std::fs::write(&file, "prefix = \"C-a\"\n").unwrap();
    h.prefixed_char('r').await;
    assert!(h.app.keymap.is_prefix(&ctrl_a()), "{}", said(&h));
    assert_eq!(h.app.config.prefix, "C-a");
    assert_eq!(said(&h), "config reloaded");

    std::fs::write(&file, "prefix = [").unwrap();
    h.event(crossterm::event::Event::Key(ctrl_a())).await;
    h.key(KeyCode::Char('r')).await;
    assert!(said(&h).starts_with("reload-config:"), "{}", said(&h));
    assert!(h.app.keymap.is_prefix(&ctrl_a()));
    assert_eq!(h.app.config.prefix, "C-a");
}

#[tokio::test]
async fn a_file_that_sources_itself_stops_at_the_nesting_limit() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("loop.conf");
    std::fs::write(&file, format!("source-file {}\n", file.display())).unwrap();
    h.command(&format!("source-file {}", file.display())).await;
    assert_eq!(said(&h), "source-file: nested more than 16 deep");
}
