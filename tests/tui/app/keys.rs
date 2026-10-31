//! Typed keys, pastes, focus reports and the prefix key reaching the active pane.
use crossterm::event::{Event, KeyCode};

use mpx::config::Config;
use mpx::layout::Direction;

use super::support::{ECHO, Harness, MAIN, split, wait_for};

fn bracketed_paste_script() -> String {
    format!(r"printf '\033[?2004h'; {ECHO}")
}

fn focus_script() -> String {
    format!(r"printf '\033[?1004h'; {ECHO}")
}

#[tokio::test]
async fn typed_keys_reach_the_active_pane() {
    let mut h = Harness::idle().await;
    h.type_str("hi").await;
    h.key(KeyCode::Enter).await;
    let logs = wait_for(&mut h.probe, MAIN, "hi\r").await;
    assert!(logs.ends_with("hi\r"));
}

#[tokio::test]
async fn paste_is_bracketed_only_for_panes_that_enabled_it() {
    let mut h = Harness::start(Config::default(), &bracketed_paste_script()).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    h.until("bracketed paste enabled", |app| {
        app.modes_of(MAIN).bracketed_paste
    })
    .await;

    h.event(Event::Paste("hello".into())).await;
    wait_for(&mut h.probe, MAIN, "\x1b[200~hello\x1b[201~").await;

    h.prefixed_char('o').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    h.event(Event::Paste("world".into())).await;
    let logs = wait_for(&mut h.probe, "plain", "world").await;
    assert!(!logs.contains("200~"), "plain pane gets raw text");
}

#[tokio::test]
async fn focus_reports_reach_only_panes_that_requested_them() {
    let mut h = Harness::start(Config::default(), &focus_script()).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    h.until("focus events enabled", |app| {
        app.modes_of(MAIN).focus_events
    })
    .await;
    // The first frame already reports focus-in: the pane's modes arrive with its screen.
    wait_for(&mut h.probe, MAIN, "\x1b[I").await;

    h.prefixed_char('o').await;
    h.frame().await;
    wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O").await;

    h.event(Event::FocusGained).await;
    h.prefixed_char('o').await;
    h.frame().await;
    wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O\x1b[I").await;

    h.event(Event::FocusLost).await;
    let logs = wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O\x1b[I\x1b[O").await;
    assert_eq!(logs.matches("\x1b[I").count(), 2, "focus-in on each return");
    let plain = h.logs("plain").await;
    assert!(!plain.contains("\x1b[I") && !plain.contains("\x1b[O"));
}

#[tokio::test]
async fn prefix_twice_sends_the_prefix_byte() {
    let mut h = Harness::idle().await;
    h.ctrl('b').await;
    h.ctrl('b').await;
    wait_for(&mut h.probe, MAIN, "\x02").await;
}

#[tokio::test]
async fn focus_reports_survive_output_beyond_what_the_log_keeps() {
    // The request for focus events scrolls out of the pane's log before the TUI attaches, so
    // the screen rebuilt from the log alone would not know about it.
    let flood = r"head -c 2500000 /dev/zero | tr '\0' x";
    let script = format!(r"printf '\033[?1004h'; {flood}; {ECHO}");
    let mut h = Harness::start(Config::default(), &script).await;
    h.frame().await;
    assert!(h.app.modes_of(MAIN).focus_events);
    wait_for(&mut h.probe, MAIN, "\x1b[I").await;
}
