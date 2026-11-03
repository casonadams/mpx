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
        width: 80,
        height: 24,
        ..Default::default()
    }
}

/// Subscribes as `info` on a new connection. `hangup` makes the session hang up the parents of the
/// clients it detaches (`attach -x`). The session's events for it arrive on the receiver.
async fn attach(
    endpoint: &str,
    info: ClientInfo,
    exclusive: bool,
    hangup: bool,
) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(64);
    let client = connect(endpoint).await;
    tokio::spawn(async move {
        let _ = client
            .stream_as_client(info, exclusive, hangup, tx, |event| event)
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
    let _events = attach(&daemon.endpoint, terminal("/dev/ttys001"), false, false).await;
    let mut probe = connect(&daemon.endpoint).await;

    let clients = await_clients(&mut probe, 1).await;
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].name, "/dev/ttys001");
    assert_eq!(clients[0].session, SESSION);
}

#[tokio::test]
async fn exclusive_attach_detaches_the_other_clients() {
    let daemon = start().await;
    let mut first = attach(&daemon.endpoint, terminal("first"), false, false).await;
    let mut probe = connect(&daemon.endpoint).await;
    await_clients(&mut probe, 1).await;

    let _second = attach(&daemon.endpoint, terminal("second"), true, false).await;

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
    let mut target = attach(&daemon.endpoint, terminal("target"), false, false).await;
    let mut other = attach(&daemon.endpoint, terminal("other"), false, false).await;
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
    let mut events = attach(&daemon.endpoint, terminal("gone"), false, false).await;
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

/// The clients' parent process: a real child that stands in for the shell a terminal was started
/// from. `attach -x` must signal it.
#[cfg(unix)]
fn parent_process() -> std::process::Child {
    std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn a parent process")
}

/// Waits (bounded) for `child` to exit and returns its status, or None when it is still running.
#[cfg(unix)]
async fn exit_status(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    for _ in 0..100 {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        sleep(Duration::from_millis(10)).await;
    }
    None
}

/// Attaches a client owned by a parent process, and waits until the session has it.
#[cfg(unix)]
async fn attached_from_parent(
    endpoint: &str,
) -> (std::process::Child, mpsc::Receiver<StreamEvent>, Client) {
    let parent = parent_process();
    let info = ClientInfo {
        parent_pid: Some(parent.id()),
        ..terminal("first")
    };
    let first = attach(endpoint, info, false, false).await;
    let mut probe = connect(endpoint).await;
    await_clients(&mut probe, 1).await;
    (parent, first, probe)
}

#[cfg(unix)]
#[tokio::test]
async fn attach_x_sends_sighup_to_the_parent_of_each_detached_client() {
    use std::os::unix::process::ExitStatusExt;

    let daemon = start().await;
    let (mut parent, mut first, _probe) = attached_from_parent(&daemon.endpoint).await;
    let _second = attach(&daemon.endpoint, terminal("second"), true, true).await;

    assert!(matches!(
        next_event(&mut first).await,
        Some(StreamEvent::Detached)
    ));
    let status = exit_status(&mut parent)
        .await
        .expect("the parent was hung up");
    assert_eq!(status.signal(), Some(libc::SIGHUP), "{status:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn plain_exclusive_attach_leaves_the_parent_of_a_detached_client_alone() {
    let daemon = start().await;
    let (mut parent, mut first, _probe) = attached_from_parent(&daemon.endpoint).await;
    let _second = attach(&daemon.endpoint, terminal("second"), true, false).await;

    assert!(matches!(
        next_event(&mut first).await,
        Some(StreamEvent::Detached)
    ));
    assert!(
        exit_status(&mut parent).await.is_none(),
        "-d must not signal the parent"
    );
    parent.kill().expect("stop the parent");
}
