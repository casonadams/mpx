use mpx::config::{Config, Passthrough};
use mpx::layout::Direction;
use mpx::protocol::StreamEvent;

use super::support::{ECHO, Harness, split};

#[tokio::test]
async fn active_pane_passthrough_is_forwarded_to_app() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let _ = h.frame().await;
    let active = h.app.active_pane.clone();
    assert!(!active.is_empty());

    let data = "\x1b]4;0;rgb:24/2A/36\x1b\\".to_string();
    let offset = h.app.expected.get(&active).copied().unwrap_or(0);
    let next = offset + data.len() as u64;

    let event = StreamEvent::Output {
        name: active.clone(),
        data,
        offset,
        next,
    };
    h.app.apply_event(event).await;

    assert_eq!(
        h.app.passthroughs,
        vec![b"\x1b]4;0;rgb:24/2A/36\x1b\\".to_vec()]
    );
}

#[tokio::test]
async fn inactive_pane_passthrough_is_discarded() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let _ = h.frame().await;

    let event = StreamEvent::Output {
        name: "some-other-pane".to_string(),
        data: "\x1b]4;0;rgb:24/2A/36\x1b\\".to_string(),
        offset: 0,
        next: 20,
    };
    h.app.apply_event(event).await;

    assert!(h.app.passthroughs.is_empty());
}

#[tokio::test]
async fn allow_passthrough_false_blocks_forwarding() {
    let config = Config {
        allow_passthrough: Passthrough::Off,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    let _ = h.frame().await;
    let active = h.app.active_pane.clone();

    let data = "\x1b]4;0;rgb:24/2A/36\x1b\\".to_string();
    let offset = h.app.expected.get(&active).copied().unwrap_or(0);
    let next = offset + data.len() as u64;

    let event = StreamEvent::Output {
        name: active,
        data,
        offset,
        next,
    };
    h.app.apply_event(event).await;

    assert!(h.app.passthroughs.is_empty());
}

#[tokio::test]
async fn passthrough_all_forwards_from_every_pane_the_terminal_shows() {
    let config = Config {
        allow_passthrough: Passthrough::All,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    let _ = h.frame().await;
    let active = h.app.active_pane.clone();
    split(&mut h.probe, &active, "shown", Direction::Vertical).await;
    let _ = h.frame().await;
    let data = "\x1b]4;0;rgb:24/2A/36\x1b\\".to_string();
    let offset = h.app.expected.get("shown").copied().unwrap_or(0);
    let next = offset + data.len() as u64;

    let event = StreamEvent::Output {
        name: "shown".to_string(),
        data: data.clone(),
        offset,
        next,
    };
    h.app.apply_event(event).await;

    assert_eq!(h.app.passthroughs, vec![data.into_bytes()]);
}
