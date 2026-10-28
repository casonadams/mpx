//! Output pacing end to end: a terminal that falls behind pauses a flooding pane's output, and the
//! pane runs free again once that terminal goes away. (A terminal that stops reading for good is
//! covered by `holds` in tests/node/pacing.rs.)
use std::time::Duration;

use super::support::{connect, pty, start};
use mpx::client::Client;
use mpx::protocol::StreamEvent;

async fn produced(client: &mut Client, name: &str) -> u64 {
    client
        .logs(name, None, false, Some(u64::MAX))
        .await
        .expect("logs")
        .1
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_that_falls_behind_paces_a_flood_until_it_detaches() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let (tx, _held) = tokio::sync::mpsc::channel(1);
    let terminal = connect(&daemon.endpoint).await;
    let attached = tokio::spawn(async move {
        let _ = terminal.stream_logs_into((None, None), false, tx, |event| event).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    client
        .spawn(pty("flood", "sh", &["-c", "exec yes 0123456789abcdef0123456789abcdef"]))
        .await
        .expect("spawn flood");

    tokio::time::sleep(Duration::from_millis(250)).await;
    let paced = produced(&mut client, "flood").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let still = produced(&mut client, "flood").await;
    assert!(
        still - paced < 64 * 1024,
        "output kept flowing to a terminal that is not reading: {paced} -> {still}"
    );

    attached.abort();
    let _ = attached.await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let freed = produced(&mut client, "flood").await;
    assert!(
        freed - still > 1024 * 1024,
        "the pane must run free once the terminal detaches: {still} -> {freed}"
    );
    client.kill("flood").await.expect("kill flood");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flooding_pane_still_answers_requests_and_dies_on_kill_at_once() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty("flood", "sh", &["-c", "exec yes 0123456789abcdef0123456789abcdef"]))
        .await
        .expect("spawn flood");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let asked = std::time::Instant::now();
    produced(&mut client, "flood").await;
    assert!(
        asked.elapsed() < Duration::from_millis(200),
        "a request waited {:?} behind unparsed output",
        asked.elapsed()
    );
    let killed = std::time::Instant::now();
    client.kill("flood").await.expect("kill flood");
    assert!(
        killed.elapsed() < Duration::from_millis(500),
        "kill took {:?}",
        killed.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_a_pane_in_full_does_not_overflow_an_attached_terminal() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let terminal = connect(&daemon.endpoint).await;
    let attached = tokio::spawn(async move {
        let _ = terminal.stream_logs_into((None, None), true, tx, |event| event).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    // A line per write: a flood of small events, thousands for each millisecond a read could
    // hold the session.
    let lines = "while :; do seq 1 100000; done";
    client
        .spawn(pty("flood", "sh", &["-c", lines]))
        .await
        .expect("spawn flood");
    let watched = tokio::spawn(async move {
        let mut lagged = false;
        while let Some(event) = rx.recv().await {
            lagged |= matches!(event, StreamEvent::Lagged);
        }
        lagged
    });

    let until = std::time::Instant::now() + Duration::from_millis(800);
    while std::time::Instant::now() < until {
        client
            .logs("flood", None, true, None)
            .await
            .expect("plain logs");
    }
    client.kill("flood").await.expect("kill flood");
    attached.abort();
    assert!(
        !watched.await.expect("watcher"),
        "the terminal lagged while the pane was read"
    );
}
