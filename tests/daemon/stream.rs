use std::time::Duration;

use super::support::{connect, logs_until, start, task};
use mpx::protocol::{StreamEvent, WaitCondition};

/// (offset, next, data) of every output event, in arrival order.
type Chunk = (u64, u64, String);

/// Streams `name` from `since` until `done` says stop, failing the test if it takes too long.
async fn collect(
    endpoint: &str,
    name: Option<&str>,
    since: Option<u64>,
    mut done: impl FnMut(&[Chunk], &StreamEvent) -> bool,
) -> Vec<Chunk> {
    let mut sub = connect(endpoint).await;
    let mut events: Vec<Chunk> = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(4),
        sub.stream_logs(name.map(str::to_string), since, |event| {
            if let StreamEvent::Output {
                data, offset, next, ..
            } = &event
            {
                events.push((*offset, *next, data.clone()));
            }
            !done(&events, &event)
        }),
    )
    .await
    .expect("stream ended in time")
    .expect("stream subscribed");
    events
}

#[tokio::test]
async fn since_replays_the_tail_then_continues_live_without_gap_or_duplicates() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("replay", "cat", &[])).await.unwrap();
    client.send_input("replay", "one\n").await.unwrap();
    logs_until(&mut client, "replay", "one\n").await;
    let (_, mid) = client.logs("replay", None, false, None).await.unwrap();
    client.send_input("replay", "two\n").await.unwrap();
    logs_until(&mut client, "replay", "two\n").await;

    let ep = daemon.endpoint.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut writer = connect(&ep).await;
        writer.send_input("replay", "three\n").await.unwrap();
    });

    let events = collect(&daemon.endpoint, Some("replay"), Some(mid), |events, _| {
        events.iter().any(|(_, _, d)| d.contains("three"))
    })
    .await;

    assert_eq!(events[0].0, mid, "replay starts at the requested offset");
    for pair in events.windows(2) {
        assert_eq!(pair[1].0, pair[0].1, "chunks are contiguous");
    }
    let text: String = events.iter().map(|(_, _, d)| d.as_str()).collect();
    assert_eq!(text, "two\nthree\n");
}

#[tokio::test]
async fn since_without_a_node_name_is_an_error() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let err = client
        .stream_logs(None, Some(0), |_| false)
        .await
        .unwrap_err();
    assert!(err.contains("node name"), "error: {err}");
}

#[tokio::test]
async fn live_output_split_across_reads_keeps_offsets_contiguous() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task(
            "wide",
            "sh",
            &["-c", "sleep 0.3; yes 'é€😀' | head -n 5000"],
        ))
        .await
        .unwrap();

    let events = collect(&daemon.endpoint, Some("wide"), Some(0), |_, event| {
        matches!(event, StreamEvent::Exit { .. })
    })
    .await;

    assert!(events.len() > 1, "output arrived in several chunks");
    for pair in events.windows(2) {
        assert_eq!(pair[1].0, pair[0].1, "no gap between chunks");
    }
    for (offset, next, data) in &events {
        assert_eq!(
            data.len() as u64,
            next - offset,
            "data covers exactly [offset, next)"
        );
        assert!(!data.contains('\u{FFFD}'), "no split character was mangled");
    }
    let text: String = events.iter().map(|(_, _, d)| d.as_str()).collect();
    assert!(
        text.ends_with("é€😀\n"),
        "tail: {:?}",
        &text[text.len().saturating_sub(12)..]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_subscriber_is_told_it_lagged_instead_of_stalling_the_pane() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("firehose", "sh", &["-c", "while :; do echo x; done"]))
        .await
        .unwrap();

    // Stop reading for a while: the socket fills, then the subscriber's queue overflows.
    let mut sub = connect(&daemon.endpoint).await;
    let mut lagged = false;
    let mut stalled = false;
    tokio::time::timeout(
        Duration::from_secs(4),
        sub.stream_logs(None, None, |event| {
            if matches!(event, StreamEvent::Lagged) {
                lagged = true;
            }
            if !stalled {
                stalled = true;
                std::thread::sleep(Duration::from_millis(500));
            }
            !lagged
        }),
    )
    .await
    .expect("stalled subscriber was told it lagged")
    .unwrap();
    assert!(lagged);

    client.kill("firehose").await.unwrap();
    let outcome = client
        .wait("firehose", WaitCondition::Exit, Some(2_000))
        .await
        .unwrap();
    assert!(
        outcome.satisfied,
        "the firehose pane still stops on request"
    );
}
