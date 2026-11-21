//! After the daemon reports the app lagged (its queue overflowed and events were dropped), each
//! shown pane catches up on what it missed.
use crossterm::event::KeyCode;
use mpx::protocol::StreamEvent;

use super::support::{Harness, MAIN, wait_for};

/// Sends `text` to the main pane and lets the daemon echo it, but drops the events that carry it,
/// as an overflowing queue does, then reports the lag.
async fn lose_then_lag(h: &mut Harness, text: &str) {
    h.probe.send_input(MAIN, text).await.expect("send");
    wait_for(&mut h.probe, MAIN, text.trim_end()).await;
    while h.rx.try_recv().is_ok() {}
    h.app.apply_event(StreamEvent::Lagged).await;
}

fn screen(h: &Harness) -> String {
    h.app
        .vt
        .get(MAIN)
        .map(|vt| vt.snapshot())
        .unwrap_or_default()
}

#[tokio::test]
async fn output_lost_to_a_lag_is_shown_after_it() {
    let mut h = Harness::idle().await;
    h.frame().await;
    lose_then_lag(&mut h, "LOST-OUT\r").await;
    assert!(screen(&h).contains("LOST-OUT"), "{}", screen(&h));
    assert!(
        h.app.vt[MAIN].grid().history_limit == 0,
        "a live pane stays without scrollback"
    );
}

#[tokio::test]
async fn a_lag_leaves_a_copy_mode_view_as_it_was_entered() {
    let mut h = Harness::idle().await;
    h.frame().await;
    h.prefixed_char('[').await;
    lose_then_lag(&mut h, "DURING-COPY\r").await;
    assert!(
        !screen(&h).contains("DURING-COPY"),
        "copy mode shows the pane as it was entered"
    );
    h.key(KeyCode::Char('q')).await;
    h.frame().await;
    assert!(screen(&h).contains("DURING-COPY"), "leaving it catches up");
}

#[tokio::test]
async fn a_respawn_lost_to_a_lag_shows_the_new_process() {
    let mut h = Harness::idle().await;
    h.frame().await;
    h.emit(MAIN, &"old output\r".repeat(20), "old output").await;
    h.prefixed_char(':').await;
    h.type_str("respawn-pane -k sh -c 'printf AGAIN; exec cat'")
        .await;
    h.key(KeyCode::Enter).await;
    wait_for(&mut h.probe, MAIN, "AGAIN").await;
    while h.rx.try_recv().is_ok() {}
    h.app.apply_event(StreamEvent::Lagged).await;
    h.frame().await;
    assert!(screen(&h).contains("AGAIN"), "{}", screen(&h));
    assert!(!screen(&h).contains("old output"), "{}", screen(&h));
}
