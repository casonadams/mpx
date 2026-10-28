use std::time::{Duration, Instant};

use mpx::node::pacing::{Inflight, PACE_HIGH, PACE_STUCK_MS, PANE_INFLIGHT, Pacing, holds, pane_full};

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
        backlog.queued();
    }
    assert!(pacing.congested(1_000));
    backlog.sent(1_010);
    assert!(!pacing.congested(1_010), "back at the mark");
    backlog.queued();
    assert!(pacing.congested(1_020));
    drop(backlog);
    assert!(!pacing.congested(1_020), "a closed subscription no longer counts");
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
        backlog.queued();
    }
    let alive = std::sync::Arc::clone(&backlog);
    drop(mpx::node::pacing::CloseOnDrop(Some(backlog)));
    assert!(!alive.holds(1_000));
    assert!(!pacing.congested(1_000), "still referenced, but its subscription ended");
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
