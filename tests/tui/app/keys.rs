//! Typed keys, pastes, focus reports and the prefix key reaching the active pane.
use crossterm::event::{Event, KeyCode};

use mpx::config::{Config, TerminalOptions};

use super::support::{ECHO, Harness, MAIN, wait_for};

fn bracketed_paste_script() -> String {
    format!(r"printf '\033[?2004h'; {ECHO}")
}

fn focus_script() -> String {
    format!(r"printf '\033[?1004h'; {ECHO}")
}

/// Moves focus to the next pane with the prefix key and draws the frame that reports it.
async fn next_pane(h: &mut Harness) {
    h.prefixed_char('o').await;
    h.frame().await;
}

/// Waits for `MAIN` to ask for focus reports; the first frame then reports focus-in, as the
/// pane's modes arrive with its screen.
async fn first_focus_in(h: &mut Harness) {
    h.until("focus events requested", |app| {
        app.modes_of(MAIN).focus_events
    })
    .await;
    wait_for(&mut h.probe, MAIN, "\x1b[I").await;
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
    let mut h = Harness::start_split(Config::default(), &bracketed_paste_script()).await;
    h.until("bracketed paste enabled", |app| {
        app.modes_of(MAIN).bracketed_paste
    })
    .await;

    h.event(Event::Paste("hello".into())).await;
    wait_for(&mut h.probe, MAIN, "\x1b[200~hello\x1b[201~").await;

    next_pane(&mut h).await;
    assert_eq!(h.app.active_pane, "plain");
    h.event(Event::Paste("world".into())).await;
    let logs = wait_for(&mut h.probe, "plain", "world").await;
    assert!(!logs.contains("200~"), "plain pane gets raw text");
}

#[tokio::test]
async fn focus_reports_reach_only_panes_that_requested_them() {
    let config = Config {
        terminal: TerminalOptions {
            focus_events: true,
            ..TerminalOptions::default()
        },
        ..Config::default()
    };
    let mut h = Harness::start_split(config, &focus_script()).await;
    first_focus_in(&mut h).await;

    next_pane(&mut h).await;
    wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O").await;

    h.event(Event::FocusGained).await;
    next_pane(&mut h).await;
    wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O\x1b[I").await;

    h.event(Event::FocusLost).await;
    let logs = wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O\x1b[I\x1b[O").await;
    assert_eq!(logs.matches("\x1b[I").count(), 2, "focus-in on each return");
    let plain = h.logs("plain").await;
    assert!(!plain.contains("\x1b[I") && !plain.contains("\x1b[O"));
}

#[tokio::test]
async fn pane_focus_reports_without_focus_events_but_the_terminals_focus_does_not() {
    let mut h = Harness::start_split(Config::default(), &focus_script()).await;
    first_focus_in(&mut h).await;
    for terminal_focus in [Event::FocusGained, Event::FocusLost] {
        h.event(terminal_focus).await;
    }
    next_pane(&mut h).await;
    let logs = wait_for(&mut h.probe, MAIN, "\x1b[I\x1b[O").await;
    assert_eq!(
        logs.matches("\x1b[I").count(),
        1,
        "the terminal's focus is not forwarded"
    );
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
    let config = Config {
        terminal: TerminalOptions {
            focus_events: true,
            ..TerminalOptions::default()
        },
        ..Config::default()
    };
    let mut h = Harness::start(config, &script).await;
    h.frame().await;
    assert!(h.app.modes_of(MAIN).focus_events);
    wait_for(&mut h.probe, MAIN, "\x1b[I").await;
}
