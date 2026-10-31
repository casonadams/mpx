//! Display and text views driven through the app: display-panes, clock-mode, show-messages,
//! list-buffers, choose-buffer, multi-line tmux output and suspend-client.
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::tui::app::{ClockSource, Mode};

use super::support::{ECHO, Harness, MAIN, SESSION, wait_for};

/// What opens an overlay or reports on the status row: a `:` command line, or a key after the prefix.
enum Opener {
    Command(&'static str),
    Prefixed(char),
}

#[tokio::test]
async fn display_panes_draws_numbers_and_a_digit_selects_that_pane() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.split_side("side").await;
    assert_eq!(h.app.active_pane, MAIN);

    h.command("display-panes -d 0").await;
    assert!(matches!(h.app.mode, Mode::Panes { until: None }));
    let shown = h.frame().await;
    assert!(shown.contains('█'), "numbers are drawn large:\n{shown}");

    h.key(KeyCode::Char('1')).await;
    assert!(h.mode_is_normal());
    h.frame().await;
    assert_eq!(h.app.active_pane, "side", "pane 1 is the second pane");
}

#[tokio::test]
async fn a_digit_or_other_key_only_dismisses_the_numbers() {
    for key in [KeyCode::Char('7'), KeyCode::Esc] {
        let mut h = Harness::idle().await;
        h.command("display-panes -d 0").await;
        h.key(key).await;
        assert!(h.mode_is_normal(), "{key:?} dismisses the numbers");
        assert_eq!(h.app.active_pane, MAIN);
    }
}

#[tokio::test]
async fn display_panes_times_out_after_its_duration_or_the_configured_default() {
    let mut h = Harness::idle().await;

    h.command("display-panes -d 1000").await;
    assert!(h.app.display_deadline().is_some());
    h.app.expire_panes(Instant::now());
    assert!(matches!(h.app.mode, Mode::Panes { .. }), "not due yet");
    h.app.expire_panes(Instant::now() + Duration::from_secs(2));
    assert!(h.mode_is_normal());

    h.command("display-panes").await;
    assert!(
        matches!(h.app.mode, Mode::Panes { until: Some(_) }),
        "no -d takes display-panes-time"
    );
}

/// Whether the app is in the mode an overlay opens.
type ModeCheck = fn(&Mode) -> bool;

/// Each overlay opens on its command or key, draws its text, and closes on its own key.
#[tokio::test]
async fn overlays_open_show_their_text_and_close_on_their_key() {
    let cases: [(Opener, ModeCheck, &str, KeyCode); 2] = [
        (
            Opener::Command("clock-mode"),
            |mode| matches!(mode, Mode::Clock { pane } if pane == MAIN),
            "█",
            KeyCode::Char('a'),
        ),
        (
            Opener::Prefixed('?'),
            |mode| matches!(mode, Mode::Overlay(_)),
            "new-window",
            KeyCode::Esc,
        ),
    ];
    for (opener, shows, text, close) in cases {
        let mut h = Harness::start(Config::default(), ECHO).await;
        h.app.clock = ClockSource::Fixed(9, 41);
        h.frame().await;
        let shown = match opener {
            Opener::Command(line) => h.command_frame(line).await,
            Opener::Prefixed(c) => h.prefixed_frame(c).await,
        };
        assert!(shown.contains(text), "{text:?} missing from:\n{shown}");
        h.closes_with(shows, close).await;
    }
}

/// Commands and keys that report on the status row, with the message each leaves there.
#[tokio::test]
async fn commands_and_keys_report_their_outcome_on_the_status_row() {
    let cases = [
        (Opener::Command("clock-mode -t nowhere"), "no pane nowhere"),
        (Opener::Command("display-message window #{window_index}"), "window 0"),
        (Opener::Prefixed(')'), "no other session"),
    ];
    for (opener, want) in cases {
        let mut h = Harness::idle().await;
        let message = match opener {
            Opener::Command(line) => h.command_message(line).await,
            Opener::Prefixed(c) => h.prefixed_message(c).await,
        };
        assert_eq!(message.as_deref(), Some(want));
        assert!(h.mode_is_normal());
        assert_eq!(h.app.session, SESSION);
    }
}

#[tokio::test]
async fn show_messages_lists_the_status_row_messages() {
    let mut h = Harness::idle().await;
    h.app.say("first note");
    h.app.say("second note");

    let shown = h.command_frame("show-messages").await;
    assert!(
        shown.contains("first note") && shown.contains("second note"),
        "{shown}"
    );
    h.closes_with(|mode| matches!(mode, Mode::Overlay(_)), KeyCode::Char('q'))
        .await;
}

#[tokio::test]
async fn the_message_log_keeps_the_last_hundred_messages() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    for i in 0..105 {
        h.app.say(format!("note {i}"));
    }
    assert_eq!(h.app.message_log.len(), 100);
    assert_eq!(
        h.app.message_log.first().map(String::as_str),
        Some("note 5")
    );
    assert_eq!(
        h.app.message_log.last().map(String::as_str),
        Some("note 104")
    );
}

#[tokio::test]
async fn list_buffers_shows_each_buffer_with_its_size_and_sample() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.probe
        .set_buffer(Some("alpha"), "hello\nworld")
        .await
        .expect("set buffer");
    h.frame().await;

    h.command("list-buffers").await;
    assert!(matches!(h.app.mode, Mode::Overlay(_)));
    let shown = h.frame().await;
    assert!(
        shown.contains("alpha: 11 bytes: \"hello\\nworld\""),
        "{shown}"
    );
}

#[tokio::test]
async fn choose_buffer_pastes_the_highlighted_buffer_and_deletes_on_d() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.probe
        .set_buffer(Some("older"), "old text")
        .await
        .expect("set buffer");
    h.probe
        .set_buffer(Some("newer"), "fresh text")
        .await
        .expect("set buffer");
    h.frame().await;

    h.command("choose-buffer").await;
    assert!(matches!(h.app.mode, Mode::Overlay(_)));
    let shown = h.frame().await;
    assert!(
        shown.contains("newer") && shown.contains("older"),
        "{shown}"
    );

    h.key(KeyCode::Enter).await;
    assert!(h.mode_is_normal());
    let output = wait_for(&mut h.probe, MAIN, "fresh text").await;
    assert!(output.contains("fresh text"), "the newest buffer is pasted");

    h.command("choose-buffer").await;
    h.key(KeyCode::Char('d')).await;
    assert!(
        matches!(h.app.mode, Mode::Overlay(_)),
        "deleting keeps the list open"
    );
    let names: Vec<String> = h
        .probe
        .list_buffers()
        .await
        .expect("list buffers")
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, vec!["older".to_string()]);
}

#[cfg(unix)]
#[tokio::test]
async fn suspend_client_asks_the_event_loop_to_stop_the_process() {
    let mut h = Harness::idle().await;

    h.command("suspend-client").await;
    assert!(h.mode_is_normal());
    assert!(h.app.suspend_requested);
}

#[test]
fn the_system_clock_reads_a_valid_local_time() {
    let (hour, minute) = ClockSource::System.hour_minute();
    assert!(hour < 24 && minute < 60, "{hour}:{minute}");
}
