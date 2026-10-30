use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use super::support::{connect, start};
use mpx::protocol::{PopupExit, PopupOpts, PopupSpec, SpawnOpts, StreamEvent};

const WAIT: Duration = Duration::from_secs(5);

fn popup(command: &str, args: &[&str], exit: PopupExit, wait: bool) -> PopupOpts {
    PopupOpts {
        spawn: SpawnOpts {
            command: command.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            pty: true,
            ..SpawnOpts::default()
        },
        spec: PopupSpec::default(),
        exit,
        wait,
    }
}

/// Terminal-view events of the session (popup open and close, exits), in arrival order.
async fn terminal_events(endpoint: &str) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(256);
    let sub = connect(endpoint).await;
    tokio::spawn(async move {
        let _ = sub
            .stream_logs_into((None, None), true, tx, |event| event)
            .await;
    });
    // The subscription is registered by the spawned task: give it time before the test opens popups.
    tokio::time::sleep(Duration::from_millis(100)).await;
    rx
}

/// Waits (bounded) for the first event that `ready` accepts.
async fn until_event(
    rx: &mut mpsc::Receiver<StreamEvent>,
    what: &str,
    mut ready: impl FnMut(&StreamEvent) -> bool,
) -> StreamEvent {
    tokio::time::timeout(WAIT, async {
        loop {
            let event = rx.recv().await.expect("event stream open");
            if ready(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no event: {what}"))
}

/// The name of the popup pane announced by a `PopupOpen`.
async fn opened(rx: &mut mpsc::Receiver<StreamEvent>) -> String {
    match until_event(rx, "popup open", |e| {
        matches!(e, StreamEvent::PopupOpen { .. })
    })
    .await
    {
        StreamEvent::PopupOpen { name, .. } => name,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_popup_runs_its_program_and_closes_with_its_status() {
    let daemon = start().await;
    let mut events = terminal_events(&daemon.endpoint).await;
    let mut client = connect(&daemon.endpoint).await;

    let status = tokio::time::timeout(
        WAIT,
        client.popup(popup("sh", &["-c", "exit 3"], PopupExit::Close, true)),
    )
    .await
    .expect("the waiting popup is answered")
    .expect("popup accepted");

    assert_eq!(status, Some(3), "the program's status is returned");
    let name = opened(&mut events).await;
    assert!(name.starts_with("popup-"), "{name}");
    until_event(
        &mut events,
        "popup closed",
        |e| matches!(e, StreamEvent::PopupClose { name: closed } if *closed == name),
    )
    .await;
    let nodes = client.list().await.expect("list");
    assert!(
        nodes.iter().all(|n| n.name != name),
        "the closed popup's pane is removed"
    );
}

#[tokio::test]
async fn an_open_popup_is_hidden_from_the_node_list_and_is_the_only_one() {
    let daemon = start().await;
    let mut events = terminal_events(&daemon.endpoint).await;
    let mut client = connect(&daemon.endpoint).await;

    client
        .popup(popup("sleep", &["30"], PopupExit::Stay, false))
        .await
        .expect("popup accepted");
    let name = opened(&mut events).await;

    let nodes = client.list().await.expect("list");
    assert!(
        nodes.iter().all(|n| n.name != name),
        "popups are not listed as panes"
    );
    let refused = client
        .popup(popup("sh", &["-c", "true"], PopupExit::Stay, false))
        .await
        .expect_err("a second popup is refused");
    assert!(refused.contains("already open"), "{refused}");

    client.close_popup().await.expect("close");
    until_event(&mut events, "popup closed", |e| {
        matches!(e, StreamEvent::PopupClose { .. })
    })
    .await;
}

#[tokio::test]
async fn a_popup_that_fails_stays_open_under_exit_on_success_until_it_is_closed() {
    let daemon = start().await;
    let mut events = terminal_events(&daemon.endpoint).await;
    let mut client = connect(&daemon.endpoint).await;
    let endpoint = daemon.endpoint.clone();
    let waiting = tokio::spawn(async move {
        let mut caller = connect(&endpoint).await;
        caller
            .popup(popup(
                "sh",
                &["-c", "exit 1"],
                PopupExit::CloseOnSuccess,
                true,
            ))
            .await
    });

    let name = opened(&mut events).await;
    until_event(
        &mut events,
        "failed program exits",
        |e| matches!(e, StreamEvent::Exit { name: exited, .. } if *exited == name),
    )
    .await;
    // The exit happened, but the popup stays: a second popup is still refused.
    let refused = client
        .popup(popup("sh", &["-c", "true"], PopupExit::Stay, false))
        .await
        .expect_err("the failed popup is still open");
    assert!(refused.contains("already open"), "{refused}");
    assert!(
        !waiting.is_finished(),
        "the waiting caller is still blocked"
    );

    client.close_popup().await.expect("close");
    let status = tokio::time::timeout(WAIT, waiting)
        .await
        .expect("the waiting caller is answered")
        .expect("task joined")
        .expect("popup answered");
    assert_eq!(status, Some(1), "the status of the failed program");
}

#[tokio::test]
async fn closing_a_running_popup_stops_its_program() {
    let daemon = start().await;
    let mut events = terminal_events(&daemon.endpoint).await;
    let endpoint = daemon.endpoint.clone();
    let waiting = tokio::spawn(async move {
        let mut caller = connect(&endpoint).await;
        caller
            .popup(popup("sleep", &["30"], PopupExit::Stay, true))
            .await
    });
    let mut client = connect(&daemon.endpoint).await;
    let _ = opened(&mut events).await;

    let started = Instant::now();
    client.close_popup().await.expect("close");
    let _ = tokio::time::timeout(WAIT, waiting)
        .await
        .expect("the waiting caller is answered")
        .expect("task joined")
        .expect("popup answered");
    assert!(
        started.elapsed() < WAIT,
        "a killed program does not hold the popup"
    );
    until_event(&mut events, "popup closed", |e| {
        matches!(e, StreamEvent::PopupClose { .. })
    })
    .await;
}

#[tokio::test]
async fn a_terminal_that_attaches_while_a_popup_is_open_is_told_of_it() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .popup(popup("sleep", &["30"], PopupExit::Stay, false))
        .await
        .expect("popup accepted");

    let mut late = terminal_events(&daemon.endpoint).await;
    let name = opened(&mut late).await;
    assert!(name.starts_with("popup-"), "{name}");
    client.close_popup().await.expect("close");
}
