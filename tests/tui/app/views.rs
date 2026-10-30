//! Display and text views driven through the app: display-panes, clock-mode, show-messages,
//! list-buffers, choose-buffer, multi-line tmux output and suspend-client.
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::layout::Direction;
use mpx::tui::app::{ClockSource, Mode};

use super::support::{ECHO, Harness, MAIN, split, wait_for};

/// Types `line` at the `:` prompt and submits it.
async fn type_command(h: &mut Harness, line: &str) {
    h.prefixed_char(':').await;
    h.type_str(line).await;
    h.key(KeyCode::Enter).await;
}

#[tokio::test]
async fn display_panes_draws_numbers_and_a_digit_selects_that_pane() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    h.app.refresh_nodes().await;
    assert_eq!(h.app.active_pane, MAIN);

    type_command(&mut h, "display-panes -d 0").await;
    assert!(matches!(h.app.mode, Mode::Panes { until: None }));
    let shown = h.frame().await;
    assert!(shown.contains('█'), "numbers are drawn large:\n{shown}");

    h.key(KeyCode::Char('1')).await;
    assert!(h.mode_is_normal());
    h.frame().await;
    assert_eq!(h.app.active_pane, "side", "pane 1 is the second pane");
}

#[tokio::test]
async fn a_digit_without_a_pane_or_any_other_key_only_dismisses_the_numbers() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;

    type_command(&mut h, "display-panes -d 0").await;
    h.key(KeyCode::Char('7')).await;
    assert!(h.mode_is_normal());
    assert_eq!(h.app.active_pane, MAIN);

    type_command(&mut h, "display-panes -d 0").await;
    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn display_panes_times_out_after_its_duration_or_the_configured_default() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;

    type_command(&mut h, "display-panes -d 1000").await;
    assert!(h.app.display_deadline().is_some());
    h.app.expire_panes(Instant::now());
    assert!(matches!(h.app.mode, Mode::Panes { .. }), "not due yet");
    h.app.expire_panes(Instant::now() + Duration::from_secs(2));
    assert!(h.mode_is_normal());

    type_command(&mut h, "display-panes").await;
    assert!(
        matches!(h.app.mode, Mode::Panes { until: Some(_) }),
        "no -d takes display-panes-time"
    );
}

#[tokio::test]
async fn clock_mode_fills_the_active_pane_until_a_key_is_pressed() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.app.clock = ClockSource::Fixed(9, 41);
    h.frame().await;

    type_command(&mut h, "clock-mode").await;
    assert!(matches!(&h.app.mode, Mode::Clock { pane } if pane == MAIN));
    let shown = h.frame().await;
    assert!(shown.contains('█'), "the digits are drawn:\n{shown}");

    h.key(KeyCode::Char('a')).await;
    assert!(h.mode_is_normal(), "any key ends clock mode");
}

#[tokio::test]
async fn clock_mode_names_a_missing_target_on_the_status_row() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    type_command(&mut h, "clock-mode -t nowhere").await;
    assert!(h.mode_is_normal());
    assert_eq!(h.app.live_message(), Some("no pane nowhere"));
}

#[tokio::test]
async fn show_messages_lists_the_status_row_messages() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.app.say("first note");
    h.app.say("second note");

    type_command(&mut h, "show-messages").await;
    assert!(matches!(h.app.mode, Mode::Overlay(_)));
    let shown = h.frame().await;
    assert!(
        shown.contains("first note") && shown.contains("second note"),
        "{shown}"
    );

    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
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

    type_command(&mut h, "list-buffers").await;
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

    type_command(&mut h, "choose-buffer").await;
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

    type_command(&mut h, "choose-buffer").await;
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

#[tokio::test]
async fn multi_line_tmux_output_opens_a_text_overlay() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;

    type_command(&mut h, "list-panes").await;
    assert!(
        matches!(h.app.mode, Mode::Overlay(_)),
        "two lines open the overlay"
    );
    let shown = h.frame().await;
    assert!(
        shown.contains("list-panes"),
        "titled with the command:\n{shown}"
    );
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn one_line_of_tmux_output_stays_on_the_status_row() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;

    type_command(&mut h, "list-panes").await;
    assert!(h.mode_is_normal());
    assert!(
        h.app.live_message().is_some(),
        "the line is the status message"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn suspend_client_asks_the_event_loop_to_stop_the_process() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;

    type_command(&mut h, "suspend-client").await;
    assert!(h.mode_is_normal());
    assert!(h.app.suspend_requested);
}

#[test]
fn the_system_clock_reads_a_valid_local_time() {
    let (hour, minute) = ClockSource::System.hour_minute();
    assert!(hour < 24 && minute < 60, "{hour}:{minute}");
}
