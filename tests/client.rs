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
