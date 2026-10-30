//! Typed input and paste reach every pane of a window with synchronized input on.
use mpx::config::Config;
use mpx::layout::Direction;

use super::support::{ECHO, Harness, split, wait_for};

#[tokio::test]
async fn synchronized_window_sends_typed_input_and_paste_to_every_pane() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let _ = h.frame().await;
    let main = h.app.active_pane.clone();
    split(&mut h.probe, &main, "second", Direction::Vertical).await;
    h.probe.set_synchronized(None, true).await.unwrap();
    let _ = h.frame().await;

    h.type_str("hi").await;
    let _ = h.app.on_paste("yo").await;
    for pane in [main.as_str(), "second"] {
        wait_for(&mut h.probe, pane, "hi").await;
        wait_for(&mut h.probe, pane, "yo").await;
    }
}

#[tokio::test]
async fn typed_input_stays_in_the_active_pane_without_synchronized_input() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let _ = h.frame().await;
    let main = h.app.active_pane.clone();
    split(&mut h.probe, &main, "second", Direction::Vertical).await;
    let _ = h.frame().await;

    h.type_str("solo").await;
    wait_for(&mut h.probe, &main, "solo").await;
    let other = h.probe.screen("second").await.unwrap();
    assert!(!other.contains("solo"), "{other}");
}
