use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, timeout};

use mpx::client::Client;
use mpx::protocol::{ClientInfo, ClientTarget, StreamEvent};

use super::support::{SESSION, connect, start};

const WAIT: Duration = Duration::from_secs(2);

fn terminal(name: &str) -> ClientInfo {
    ClientInfo {
        name: name.into(),
        session: String::new(),
        width: 80,
        height: 24,
        readonly: false,
        created_ms: 0,
        activity_ms: 0,
    }
}

/// Subscribes as `info` on a new connection; the session's events for it arrive on the receiver.
async fn attach(endpoint: &str, info: ClientInfo, exclusive: bool) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(64);
    let client = connect(endpoint).await;
    tokio::spawn(async move {
        let _ = client
            .stream_as_client(info, exclusive, tx, |event| event)
            .await;
    });
    rx
}

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

/// The next event that is not a layout notice (a client's own registration sends one).
async fn next_event(events: &mut mpsc::Receiver<StreamEvent>) -> Option<StreamEvent> {
    loop {
        match timeout(WAIT, events.recv()).await.expect("event in time") {
            Some(StreamEvent::Layout) => continue,
            other => return other,
        }
    }
}

#[tokio::test]
async fn a_terminal_subscription_registers_a_client_of_its_session() {
    let daemon = start().await;
    let _events = attach(&daemon.endpoint, terminal("/dev/ttys001"), false).await;
    let mut probe = connect(&daemon.endpoint).await;

    let clients = await_clients(&mut probe, 1).await;
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].name, "/dev/ttys001");
    assert_eq!(clients[0].session, SESSION);
}

#[tokio::test]
async fn exclusive_attach_detaches_the_other_clients() {
    let daemon = start().await;
    let mut first = attach(&daemon.endpoint, terminal("first"), false).await;
    let mut probe = connect(&daemon.endpoint).await;
    await_clients(&mut probe, 1).await;

    let _second = attach(&daemon.endpoint, terminal("second"), true).await;

    assert!(matches!(
        next_event(&mut first).await,
        Some(StreamEvent::Detached)
    ));
    let clients = await_clients(&mut probe, 1).await;
    assert_eq!(
        clients.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["second"]
    );
}

#[tokio::test]
async fn client_commands_reach_only_the_named_client() {
    let daemon = start().await;
    let mut target = attach(&daemon.endpoint, terminal("target"), false).await;
    let mut other = attach(&daemon.endpoint, terminal("other"), false).await;
    let mut probe = connect(&daemon.endpoint).await;
    await_clients(&mut probe, 2).await;

    probe
        .client_command("target", "switch-client -n")
        .await
        .expect("command");

    match next_event(&mut target).await {
        Some(StreamEvent::Run { line }) => assert_eq!(line, "switch-client -n"),
        other => panic!("expected the command, got {other:?}"),
    }
    let received = timeout(Duration::from_millis(100), other.recv()).await;
    assert!(
        !matches!(received, Ok(Some(StreamEvent::Run { .. }))),
        "{received:?}"
    );
}

#[tokio::test]
async fn detaching_a_named_client_ends_its_stream() {
    let daemon = start().await;
    let mut events = attach(&daemon.endpoint, terminal("gone"), false).await;
    let mut probe = connect(&daemon.endpoint).await;
    await_clients(&mut probe, 1).await;

    probe
        .detach_clients(ClientTarget::Named("gone".into()))
        .await
        .expect("detach");

    assert!(matches!(
        next_event(&mut events).await,
        Some(StreamEvent::Detached)
    ));
    assert!(await_clients(&mut probe, 0).await.is_empty());
}

#[tokio::test]
async fn naming_an_unattached_client_is_an_error() {
    let daemon = start().await;
    let mut probe = connect(&daemon.endpoint).await;

    let err = probe
        .detach_clients(ClientTarget::Named("nobody".into()))
        .await
        .expect_err("no such client");
    assert!(err.contains("can't find client: nobody"), "{err}");
}
