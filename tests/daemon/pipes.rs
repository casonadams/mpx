use std::time::{Duration, Instant};

use super::support::{connect, start};
use mpx::client::PipeReadArgs;
use mpx::protocol::StreamEvent;

#[tokio::test]
async fn writes_are_sequenced_and_reads_resume_after_a_sequence() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.create_pipe("bus", None).await.unwrap();
    assert_eq!(client.write_pipe("bus", "one").await.unwrap(), 1);
    assert_eq!(client.write_pipe("bus", "two").await.unwrap(), 2);

    let (all, next) = client
        .read_pipe("bus", PipeReadArgs::default())
        .await
        .unwrap();
    assert_eq!(
        all.iter().map(|m| m.data.as_str()).collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert_eq!(next, 2);

    let (after, next) = client
        .read_pipe(
            "bus",
            PipeReadArgs {
                since: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].seq, 2);
    assert_eq!(next, 2);
}

#[tokio::test]
async fn consumer_cursor_is_daemon_side() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.write_pipe("cursor-bus", "a").await.unwrap();
    client.write_pipe("cursor-bus", "b").await.unwrap();

    let args = PipeReadArgs {
        consumer: Some("omp".into()),
        ..Default::default()
    };
    let (first, _) = client.read_pipe("cursor-bus", args.clone()).await.unwrap();
    assert_eq!(first.len(), 2);

    let (second, next) = client.read_pipe("cursor-bus", args.clone()).await.unwrap();
    assert!(second.is_empty());
    assert_eq!(next, 2);

    client.write_pipe("cursor-bus", "c").await.unwrap();
    let (third, _) = client.read_pipe("cursor-bus", args).await.unwrap();
    assert_eq!(
        third.iter().map(|m| m.data.as_str()).collect::<Vec<_>>(),
        vec!["c"]
    );
}

#[tokio::test]
async fn wait_ms_blocks_until_a_message_arrives() {
    let daemon = start().await;
    let mut reader = connect(&daemon.endpoint).await;
    let mut writer = connect(&daemon.endpoint).await;
    reader.create_pipe("late", None).await.unwrap();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        writer.write_pipe("late", "arrived").await.unwrap();
    });

    let started = Instant::now();
    let (messages, _) = reader
        .read_pipe(
            "late",
            PipeReadArgs {
                wait_ms: Some(2_000),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data, "arrived");
    assert!(started.elapsed() < Duration::from_millis(1_500));
}

#[tokio::test]
async fn wait_ms_returns_empty_after_the_timeout() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.create_pipe("quiet", None).await.unwrap();
    let started = Instant::now();
    let (messages, next) = client
        .read_pipe(
            "quiet",
            PipeReadArgs {
                wait_ms: Some(80),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(messages.is_empty());
    assert_eq!(next, 0);
    assert!(started.elapsed() >= Duration::from_millis(80));
}

#[tokio::test]
async fn writing_creates_missing_pipes_and_create_is_idempotent() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.write_pipe("auto", "x").await.unwrap();
    client.create_pipe("auto", Some(16)).await.unwrap();
    client.write_pipe("auto", "y").await.unwrap();

    let list = client.list_pipes().await.unwrap();
    let info = list.iter().find(|p| p.name == "auto").expect("pipe listed");
    assert_eq!(info.messages, 2);
    assert_eq!(info.next_seq, 3);
}

#[tokio::test]
async fn capacity_drops_the_oldest_messages() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.create_pipe("small", Some(8)).await.unwrap();
    for data in ["aaaa", "bbbb", "cccc"] {
        client.write_pipe("small", data).await.unwrap();
    }
    let (messages, _) = client
        .read_pipe("small", PipeReadArgs::default())
        .await
        .unwrap();
    assert_eq!(
        messages.iter().map(|m| m.data.as_str()).collect::<Vec<_>>(),
        vec!["bbbb", "cccc"]
    );
}

#[tokio::test]
async fn a_reader_may_wait_on_a_pipe_before_any_writer_creates_it() {
    let daemon = start().await;
    let mut reader = connect(&daemon.endpoint).await;
    let mut writer = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        writer.write_pipe("later", "first").await.unwrap();
    });
    let args = PipeReadArgs {
        wait_ms: Some(2000),
        ..PipeReadArgs::default()
    };
    let (messages, next) = reader.read_pipe("later", args).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data, "first");
    assert_eq!(next, 1);
}

#[tokio::test]
async fn subscribers_receive_pipe_messages() {
    let daemon = start().await;
    let mut sub = connect(&daemon.endpoint).await;
    let mut writer = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        writer.write_pipe("events", "pushed").await.unwrap();
    });

    let mut got = None;
    sub.subscribe_pipe("events", |event| match event {
        StreamEvent::Pipe { message, .. } => {
            got = Some(message.data);
            false
        }
        _ => true,
    })
    .await
    .unwrap();
    assert_eq!(got.as_deref(), Some("pushed"));
}
