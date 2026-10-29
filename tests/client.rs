//! The client against a daemon from before binary frames: it asks for them, gets a plain `Ok`, and
//! reads the stream as JSON lines.
#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use tokio_util::codec::{Framed, LinesCodec};

use mpx::client::Client;
use mpx::ipc::IpcListener;
use mpx::protocol::{DaemonMessage, Response, StreamEvent};

#[tokio::test]
async fn a_daemon_without_binary_frames_streams_json_lines_to_a_new_client() {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = mpx::ipc::endpoint_in(dir.path(), "old");
    let listener = IpcListener::bind(&endpoint).expect("bind");
    let old_daemon = tokio::spawn(async move {
        let stream = listener.accept().await.expect("accept");
        let mut lines = Framed::new(stream, LinesCodec::new());
        let request = lines.next().await.expect("request").expect("line");
        let reply = |message: DaemonMessage| serde_json::to_string(&message).expect("json");
        lines
            .send(reply(DaemonMessage::Response(Response::Ok)))
            .await
            .expect("ok");
        let event = StreamEvent::Output {
            name: "p".to_string(),
            data: "\x1b[1mold\x1b[0m é".to_string(),
            offset: 0,
            next: 16,
        };
        lines
            .send(reply(DaemonMessage::Event(event)))
            .await
            .expect("event");
        request
    });

    let client = Client::connect(&endpoint).await.expect("connect");
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let _ = client.stream_logs_into((None, None), false, tx, |event| event).await;
    });
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("an event in time")
        .expect("an event");
    let request = old_daemon.await.expect("old daemon");
    assert!(request.contains("\"binary\":true"), "{request}");
    match event {
        StreamEvent::Output { data, next, .. } => {
            assert_eq!(data, "\x1b[1mold\x1b[0m é");
            assert_eq!(next, 16);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn an_event_from_a_newer_daemon_is_skipped_and_the_stream_goes_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = mpx::ipc::endpoint_in(dir.path(), "new");
    let listener = IpcListener::bind(&endpoint).expect("bind");
    let newer_daemon = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let stream = listener.accept().await.expect("accept");
        let mut lines = Framed::new(stream, LinesCodec::new());
        lines.next().await.expect("request").expect("line");
        let accepted = DaemonMessage::Response(Response::BinaryStream);
        lines
            .send(serde_json::to_string(&accepted).expect("json"))
            .await
            .expect("binary stream");
        let mut stream = lines.into_inner();
        let output = StreamEvent::Output {
            name: "p".to_string(),
            data: "after".to_string(),
            offset: 0,
            next: 5,
        };
        let mut known = Vec::new();
        mpx::wire::encode_event(&output, &mut known).expect("encode");
        let unknown = b"\0{\"event\":\"from_the_future\"}".to_vec();
        for payload in [unknown, known] {
            let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&payload);
            stream.write_all(&frame).await.expect("frame");
        }
        stream
    });

    let client = Client::connect(&endpoint).await.expect("connect");
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let _ = client.stream_logs_into((None, None), true, tx, |event| event).await;
    });
    let _held_open = newer_daemon.await.expect("daemon");
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("an event in time")
        .expect("the stream stayed open");
    assert!(matches!(event, StreamEvent::Output { data, .. } if data == "after"));
}

#[tokio::test]
async fn a_request_an_older_daemon_does_not_know_says_to_restart_the_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = mpx::ipc::endpoint_in(dir.path(), "older");
    let listener = IpcListener::bind(&endpoint).expect("bind");
    tokio::spawn(async move {
        let stream = listener.accept().await.expect("accept");
        let mut lines = Framed::new(stream, LinesCodec::new());
        lines.next().await.expect("request").expect("line");
        let refused = Response::Error(
            "Invalid request JSON: unknown variant `SpreadLayout`, expected one of `Ping`".into(),
        );
        let reply = serde_json::to_string(&DaemonMessage::Response(refused)).expect("json");
        lines.send(reply).await.expect("reply");
    });
    let mut client = Client::connect(&endpoint).await.expect("connect");
    let error = client.spread_layout().await.expect_err("refused");
    assert!(error.contains("mpx kill-server, then mpx restore"), "{error}");
}
