//! Clients of a session as the app sees them: `attach -r` keeps keys out of the panes, and the
//! `choose-client` picker detaches the client it highlights.
use std::time::Duration;

use crossterm::event::KeyCode;
use tokio::time::{Instant, sleep, timeout};

use mpx::client::Client;
use mpx::config::Config;
use mpx::protocol::{ClientInfo, StreamEvent};
use mpx::tui::app::AttachOpts;

use super::support::{ECHO, Harness, MAIN, SESSION, open_app, run_line, wait_for};

const WAIT: Duration = Duration::from_secs(2);

/// The session's clients once there are `count` of them (bounded; the caller checks the result).
async fn await_clients(probe: &mut Client, count: usize) -> Vec<ClientInfo> {
    let deadline = Instant::now() + WAIT;
    loop {
        let clients = probe.list_clients().await.expect("list clients");
        if clients.len() == count || Instant::now() >= deadline {
            return clients;
        }
        sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_read_only_client_sends_no_keys_to_the_panes() {
    let attach = AttachOpts {
        read_only: true,
        ..AttachOpts::default()
    };
    let mut h = Harness::start_with(Config::default(), ECHO, attach, None).await;
    h.frame().await;

    h.type_str("hidden").await;
    h.key(KeyCode::Enter).await;
    // Keys reach the daemon in order, so a marker sent after them shows whether they arrived.
    h.probe.send_input(MAIN, "marker\r").await.expect("marker");
    // The text alone is awaited: ConPTY may put escape sequences between echoed text and its
    // carriage return, so "marker\r" need not be contiguous in the pane's byte stream.
    let logs = wait_for(&mut h.probe, MAIN, "marker").await;
    assert!(!logs.contains("hidden"), "{logs}");
}

#[tokio::test]
async fn a_read_only_client_is_refused_commands_that_change_the_session() {
    let attach = AttachOpts {
        read_only: true,
        ..AttachOpts::default()
    };
    let mut h = Harness::start_with(Config::default(), ECHO, attach, None).await;
    h.frame().await;

    let said = run_line(&mut h, "new-window").await;
    assert!(said.contains("read-only client"), "{said}");
    assert_eq!(h.windows().await.len(), 1);
}

#[tokio::test]
async fn choose_client_lists_every_client_and_detaches_the_highlighted_one() {
    let mut h = Harness::idle().await;
    let (_other, mut other_events) = open_app(
        &h.sandbox,
        SESSION,
        Config::default(),
        AttachOpts::default(),
        Some("other-client".into()),
    )
    .await;
    await_clients(&mut h.probe, 2).await;

    h.command("choose-client").await;
    let frame = h.frame().await;
    assert!(frame.contains("Clients"), "{frame}");
    assert!(frame.contains("other-client"), "{frame}");

    // `/` filters the list to the other client; Enter ends the filter, the next Enter detaches it.
    h.key(KeyCode::Char('/')).await;
    for c in "other-client".chars() {
        h.key(KeyCode::Char(c)).await;
    }
    h.key(KeyCode::Enter).await;
    h.key(KeyCode::Enter).await;

    let detached = timeout(WAIT, async {
        loop {
            match other_events.recv().await {
                Some((_, StreamEvent::Detached)) => return true,
                Some(_) => continue,
                None => return false,
            }
        }
    })
    .await;
    assert_eq!(detached, Ok(true));
    let left = await_clients(&mut h.probe, 1).await;
    assert_eq!(left.len(), 1);
    assert_ne!(left[0].name, "other-client");
}
