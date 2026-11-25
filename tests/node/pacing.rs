use std::time::{Duration, Instant};

use mpx::node::pacing::{
    Inflight, PACE_HIGH, PACE_STUCK_MS, PANE_INFLIGHT, Pacing, RELAY_QUEUE, holds, pane_full,
    poll_delay,
};
use mpx::node::sink::Step;

#[test]
fn a_terminal_holds_readers_only_while_behind_and_progressing() {
    assert!(!holds(PACE_HIGH, 1_000, 1_000), "at the mark is not behind");
    assert!(holds(PACE_HIGH + 1, 1_000, 1_000));
    assert!(holds(PACE_HIGH + 1, 1_000, 1_000 + PACE_STUCK_MS - 1));
    assert!(
        !holds(PACE_HIGH + 1, 1_000, 1_000 + PACE_STUCK_MS),
        "a terminal that stopped reading is not waited for"
    );
}

#[test]
fn pacing_follows_live_backlogs_and_forgets_dropped_ones() {
    let pacing = Pacing::default();
    let backlog = pacing.track(1_000);
    assert!(!pacing.congested(1_000));
    for _ in 0..=PACE_HIGH {
        backlog.queued(1_000);
    }
    assert!(pacing.congested(1_000));
    backlog.sent(1_010);
    assert!(!pacing.congested(1_010), "back at the mark");
    backlog.queued(1_020);
    assert!(pacing.congested(1_020));
    drop(backlog);
    assert!(
        !pacing.congested(1_020),
        "a closed subscription no longer counts"
    );
}

#[test]
fn a_backlog_that_fills_after_a_quiet_spell_is_waited_for_from_then() {
    let pacing = Pacing::default();
    let backlog = pacing.track(1_000);
    let burst = 1_000 + 10 * PACE_STUCK_MS;
    for _ in 0..=PACE_HIGH {
        backlog.queued(burst);
    }
    assert!(pacing.congested(burst + 1), "behind since the burst began");
    assert!(
        !pacing.congested(burst + PACE_STUCK_MS),
        "and given up on once stuck that long"
    );
}

#[test]
fn output_waiting_on_the_session_holds_readers_like_a_terminal_behind() {
    let pacing = Pacing::default();
    for _ in 0..=PACE_HIGH {
        pacing.relay().queued(1_000);
    }
    assert!(
        pacing.congested(1_000),
        "no terminal is behind, the session is"
    );
    pacing.relay().sent(1_010);
    assert!(!pacing.congested(1_010));
}

#[test]
fn waiting_for_room_returns_at_once_when_no_terminal_is_behind() {
    let pacing = Pacing::default();
    let _idle = pacing.track(mpx::node::now_ms());
    let start = Instant::now();
    pacing.wait_for_room();
    assert!(start.elapsed() < Duration::from_millis(50));
}

#[test]
fn a_closed_terminal_never_holds_readers() {
    let pacing = Pacing::default();
    let backlog = pacing.track(1_000);
    for _ in 0..=PACE_HIGH {
        backlog.queued(1_000);
    }
    let alive = std::sync::Arc::clone(&backlog);
    drop(mpx::node::pacing::CloseOnDrop(Some(backlog)));
    assert!(!alive.holds(1_000));
    assert!(
        !pacing.congested(1_000),
        "still referenced, but its subscription ended"
    );
}

#[test]
fn a_pane_reader_waits_once_its_actor_is_a_bound_behind() {
    assert!(!pane_full(PANE_INFLIGHT - 1));
    assert!(pane_full(PANE_INFLIGHT));
    let inflight = Inflight::default();
    for _ in 0..PANE_INFLIGHT {
        inflight.add();
    }
    assert!(inflight.full());
    inflight.done();
    assert!(!inflight.full());
    let start = Instant::now();
    inflight.wait_for_room();
    assert!(start.elapsed() < Duration::from_millis(50));
}

#[test]
fn a_blocked_reader_backs_off_to_a_ceiling() {
    assert_eq!(poll_delay(0), Duration::from_micros(250));
    assert_eq!(poll_delay(1), Duration::from_micros(500));
    assert!(poll_delay(3) > poll_delay(2));
    assert_eq!(poll_delay(10), Duration::from_millis(4));
    assert_eq!(poll_delay(u32::MAX), Duration::from_millis(4));
}

#[test]
fn a_stalled_session_relay_drops_output_and_reports_lag_once() {
    let pacing = Pacing::default();
    let delivered = (0..RELAY_QUEUE * 2)
        .map(|_| pacing.offer_relay(1_000))
        .collect::<Vec<_>>();
    assert!(delivered.iter().filter(|s| **s == Step::Deliver).count() < RELAY_QUEUE);
    assert_eq!(delivered.iter().filter(|s| **s == Step::Lagged).count(), 1);
    assert_eq!(delivered.last(), Some(&Step::Drop));
    assert!(pacing.relay().len() < RELAY_QUEUE);

    while !pacing.relay().is_empty() {
        pacing.relay().sent(1_001);
    }
    assert_eq!(pacing.offer_relay(1_002), Step::Deliver);
}
