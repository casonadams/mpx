//! Stepping between sessions in the same socket dir with `(` and `)`.
use mpx::config::Config;

use super::support::{ECHO, Harness, MAIN, SESSION, connect, window};

/// Serves session `name` next to the harness's and gives it a window, as another client would.
async fn serve_with_window(h: &Harness, name: &str) -> mpx::client::Client {
    h.sandbox.serve(name, &Config::default()).await;
    let mut client = connect(&h.sandbox.endpoint(name)).await;
    window(&mut client, MAIN, ECHO).await;
    client
}

#[tokio::test]
async fn paren_keys_step_between_running_sessions() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    serve_with_window(&h, "other").await;
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

#[tokio::test]
async fn paren_keys_skip_a_session_with_no_panes_instead_of_ending_the_tui() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.sandbox.serve("empty", &Config::default()).await;
    h.frame().await;

    let said = h.prefixed_message(')').await.unwrap_or_default();
    assert_eq!(
        said, "no other session",
        "an empty session is not one to cycle to"
    );
    assert_eq!(h.app.session, SESSION);
    assert!(
        h.app.prepare_frame().await,
        "the TUI is still showing its own session"
    );

    serve_with_window(&h, "other").await;
    h.prefixed_char(')').await;
    assert_eq!(
        h.app.session, "other",
        "the empty one between them is skipped"
    );
    h.prefixed_char('(').await;
    assert_eq!(h.app.session, SESSION);
}

#[tokio::test]
async fn switching_to_a_session_with_no_panes_is_refused_and_leaves_the_current_one() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.sandbox.serve("empty", &Config::default()).await;
    h.frame().await;

    let said = h
        .command_message("switch-client -t empty")
        .await
        .unwrap_or_default();
    assert_eq!(said, "session 'empty' has no panes");
    assert_eq!(h.app.session, SESSION);
    assert!(h.app.prepare_frame().await);
}

#[tokio::test]
async fn a_renamed_session_is_found_by_its_new_name_even_when_another_socket_has_that_name() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    // The session on socket `kept` was renamed to `work`; an old, empty session still owns the
    // socket called `work`.
    let mut kept = serve_with_window(&h, "kept").await;
    kept.rename_session("work").await.unwrap();
    h.sandbox.serve("work", &Config::default()).await;
    h.frame().await;

    h.command("switch-client -t work").await;
    assert_eq!(h.app.session, "work");
    assert_eq!(
        h.app.endpoint,
        h.sandbox.endpoint("kept"),
        "the daemon that owns the name"
    );
    assert!(h.app.prepare_frame().await, "and it has panes to show");
}

#[tokio::test]
async fn reattaching_shows_the_name_the_session_was_renamed_to() {
    let sandbox = super::support::Sandbox::new();
    sandbox.serve("orig", &Config::default()).await;
    let mut seed = connect(&sandbox.endpoint("orig")).await;
    window(&mut seed, MAIN, ECHO).await;
    seed.rename_session("renamed").await.unwrap();

    let h = Harness::attach(sandbox, "orig", Config::default()).await;
    assert_eq!(h.app.session, "renamed");
}
