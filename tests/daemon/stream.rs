use std::time::Duration;

use super::support::{connect, logs_until, pty, start, task};
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
async fn a_stalled_subscriber_does_not_stall_the_pane() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("firehose", "sh", &["-c", "while :; do echo x; done"]))
        .await
        .unwrap();

    // A subscriber that stops reading: its socket and queue fill while the pane keeps running.
    let mut sub = connect(&daemon.endpoint).await;
    let stalled = tokio::spawn(async move {
        let _ = sub
            .stream_logs(None, None, |_| {
                std::thread::sleep(Duration::from_millis(600));
                false
            })
            .await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let before = client.logs("firehose", None, false, Some(u64::MAX)).await.unwrap().1;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = client.logs("firehose", None, false, Some(u64::MAX)).await.unwrap().1;
    assert!(after > before, "the pane kept producing: {before} -> {after}");

    client.kill("firehose").await.unwrap();
    let outcome = client
        .wait("firehose", WaitCondition::Exit, Some(2_000))
        .await
        .unwrap();
    assert!(outcome.satisfied, "the firehose pane still stops on request");
    let _ = stalled.await;
}

/// Output text and `(offset, next)` ranges of pane `name`, up to and including `marker`.
fn output_until(events: &[StreamEvent], name: &str, marker: &str) -> Option<(String, Vec<(u64, u64)>)> {
    let mut text = String::new();
    let mut ranges = Vec::new();
    for event in events {
        if let StreamEvent::Output {
            name: pane,
            data,
            offset,
            next,
        } = event
            && pane == name
        {
            text.push_str(data);
            ranges.push((*offset, *next));
            if text.contains(marker) {
                return Some((text, ranges));
            }
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_and_json_subscribers_receive_the_same_output() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;

    let (json_tx, mut json_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut json_sub = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = json_sub
            .stream_logs(None, None, |event| json_tx.send(event).is_ok())
            .await;
    });
    let (bin_tx, mut bin_rx) = tokio::sync::mpsc::channel(4096);
    let bin_sub = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = bin_sub.stream_logs_into((None, None), false, bin_tx, |event| event).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let script = r#"for i in 1 2 3; do printf '\033[31mred\033[0m é€😀 漢字 %s\r\n' "$i"; done; echo WIRE-END"#;
    client
        .spawn(pty("mix", "sh", &["-c", script]))
        .await
        .expect("spawn");

    let collect = |rx: &mut Vec<StreamEvent>| output_until(rx, "mix", "WIRE-END");
    let (mut from_json, mut from_bin) = (Vec::new(), Vec::new());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while collect(&mut from_json).is_none() || collect(&mut from_bin).is_none() {
        assert!(tokio::time::Instant::now() < deadline, "both streams end with the marker");
        tokio::select! {
            Some(event) = json_rx.recv() => from_json.push(event),
            Some(event) = bin_rx.recv() => from_bin.push(event),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }
    let (json_text, json_ranges) = collect(&mut from_json).expect("json output");
    let (bin_text, bin_ranges) = collect(&mut from_bin).expect("binary output");
    assert_eq!(bin_text, json_text);
    assert!(bin_text.contains("\x1b[31mred\x1b[0m é€😀 漢字 3"), "{bin_text:?}");
    let contiguous = |ranges: &[(u64, u64)]| ranges.windows(2).all(|w| w[0].1 == w[1].0);
    assert!(contiguous(&bin_ranges) && contiguous(&json_ranges));
    assert_eq!(bin_ranges.last().map(|r| r.1), json_ranges.last().map(|r| r.1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_active_window_subscriber_gets_only_that_windows_output_but_every_exit() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let window = |name: &str, script: &str| mpx::protocol::NewWindowOpts {
        name: Some(name.to_string()),
        spawn: pty(name, "sh", &["-c", script]),
        focus: true,
    };
    client.new_window(window("front", "cat")).await.expect("front");
    client.new_window(window("back", "cat")).await.expect("back");
    client.select_window(0).await.expect("front is active");

    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let sub = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = sub.stream_logs_into((None, None), true, tx, |event| event).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.send_input("back", "HIDDEN\n").await.expect("back input");
    client.send_input("front", "SHOWN\n").await.expect("front input");
    client.kill("back").await.expect("kill back");

    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let done = |seen: &[StreamEvent]| {
        let exit = seen
            .iter()
            .any(|e| matches!(e, StreamEvent::Exit { name, .. } if name == "back"));
        output_until(seen, "front", "SHOWN").is_some() && exit
    };
    while !done(&seen) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(event)) => seen.push(event),
            _ => panic!("missing front output or back exit: {seen:?}"),
        }
    }
    let hidden = seen
        .iter()
        .any(|e| matches!(e, StreamEvent::Output { name, .. } if name == "back"));
    assert!(!hidden, "the hidden window's output was sent");
}

#[tokio::test]
async fn a_terminal_hears_of_rearranged_windows_and_other_subscribers_do_not() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let window = |name: &str| mpx::protocol::NewWindowOpts {
        name: Some(name.to_string()),
        spawn: pty(name, "sh", &["-c", "cat"]),
        focus: true,
    };
    client.new_window(window("one")).await.expect("one");
    client.new_window(window("two")).await.expect("two");

    let (term_tx, mut term_rx) = tokio::sync::mpsc::channel(64);
    let terminal = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = terminal.stream_logs_into((None, None), true, term_tx, |e| e).await;
    });
    let (all_tx, mut all_rx) = tokio::sync::mpsc::channel(64);
    let everything = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = everything.stream_logs_into((None, None), false, all_tx, |e| e).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.select_window(0).await.expect("select window");
    let heard = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = term_rx.recv().await {
            if matches!(event, StreamEvent::Layout) {
                return true;
            }
        }
        false
    })
    .await;
    assert_eq!(heard, Ok(true), "the terminal was not told");

    client.kill("two").await.expect("kill two");
    let mut seen = Vec::new();
    while !seen.iter().any(|e| matches!(e, StreamEvent::Exit { .. })) {
        let event = tokio::time::timeout(Duration::from_secs(2), all_rx.recv()).await;
        seen.push(event.expect("events in time").expect("event"));
    }
    assert!(!seen.iter().any(|e| matches!(e, StreamEvent::Layout)), "{seen:?}");
}
