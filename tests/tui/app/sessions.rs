//! Stepping between sessions in the same socket dir with `(` and `)`.
use mpx::config::Config;

use super::support::{ECHO, Harness, MAIN, SESSION, connect, window};

#[tokio::test]
async fn paren_keys_step_between_running_sessions() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.sandbox.serve("other", &Config::default()).await;
    let mut other = connect(&h.sandbox.endpoint("other")).await;
    window(&mut other, MAIN, ECHO).await;
    h.frame().await;

    h.prefixed_char(')').await;
    assert_eq!(h.app.session, "other");
    assert!(h.frame().await.contains("[other] "));

    h.prefixed_char('(').await;
    assert_eq!(h.app.session, SESSION);
    assert!(h.frame().await.contains("[test] "));
}

#[tokio::test]
async fn the_tui_ends_when_its_daemon_goes_away() {
    let mut h = Harness::idle().await;
    let _ = h.probe.shutdown().await;
    h.app.layout.dirty = true;
    assert!(!h.app.prepare_frame().await, "nothing left to show");
}
