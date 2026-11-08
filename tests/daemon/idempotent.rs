use std::sync::Arc;
use std::time::Duration;

use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::protocol::{DaemonMessage, NodeRequest, Request, Response, WaitCondition};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};
use tokio::task::JoinHandle;

use super::support::{connect, logs_until, started_with, task};

#[tokio::test]
async fn repeated_key_pastes_once() {
    let (_daemon, mut client) = started_with(task("sink", "cat", &[])).await;

    client
        .idempotency_key("paste-1")
        .paste("sink", "hello_once", false)
        .await
        .unwrap();
    client
        .idempotency_key("paste-1")
        .paste("sink", "hello_once", false)
        .await
        .unwrap();
    client.paste("sink", "barrier_a", false).await.unwrap();
    let data = logs_until(&mut client, "sink", "barrier_a").await;
    assert_eq!(data.matches("hello_once").count(), 1, "logs: {data}");
}

#[tokio::test]
async fn concurrent_repeats_of_a_key_paste_once() {
    let (daemon, mut client) = started_with(task("racer", "cat", &[])).await;

    let mut racers = Vec::new();
    for _ in 0..2 {
        let ep = daemon.endpoint.clone();
        racers.push(tokio::spawn(async move {
            let mut c = connect(&ep).await;
            c.idempotency_key("race-1")
                .paste("racer", "race_once", false)
                .await
        }));
    }
    for racer in racers {
        racer.await.unwrap().unwrap();
    }
    client.paste("racer", "barrier_b", false).await.unwrap();
    let data = logs_until(&mut client, "racer", "barrier_b").await;
    assert_eq!(data.matches("race_once").count(), 1, "logs: {data}");
}

#[tokio::test]
async fn nested_idempotent_requests_are_rejected() {
    let (_daemon, mut client) = started_with(task("nest", "cat", &[])).await;
    let err = client
        .idempotency_key("outer")
        .stream_logs(Some("nest".to_string()), None, |_| false)
        .await
        .unwrap_err();
    assert!(err.contains("Idempotent"), "error: {err}");
}

/// One connection served by a handler on its own task, so a test can cancel it mid-request.
struct RawConn {
    write: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    handler: JoinHandle<()>,
}

fn raw_conn(state: &Arc<DaemonState>) -> RawConn {
    let (client, server) = tokio::io::duplex(1 << 16);
    let handler = tokio::spawn(handle_client(Arc::clone(state), server));
    let (read, write) = tokio::io::split(client);
    RawConn {
        write,
        lines: BufReader::new(read).lines(),
        handler,
    }
}

impl RawConn {
    async fn send(&mut self, req: &Request) {
        let mut line = serde_json::to_string(req).expect("encode");
        line.push('\n');
        self.write.write_all(line.as_bytes()).await.expect("send");
    }

    async fn reply(&mut self) -> DaemonMessage {
        let line = tokio::time::timeout(Duration::from_secs(2), self.lines.next_line())
            .await
            .expect("the daemon replies in time")
            .expect("read")
            .expect("a line");
        serde_json::from_str(&line).expect("decode")
    }
}

#[tokio::test]
async fn a_key_whose_request_is_cancelled_runs_its_repeat() {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = mpx::ipc::endpoint_in(dir.path(), "cancel");
    let state = DaemonState::new(
        "cancel".to_string(),
        endpoint,
        Config::default(),
        dir.path().join("state"),
    )
    .await;
    let never = || WaitCondition::Match {
        pattern: "never printed".to_string(),
    };

    let mut first = raw_conn(&state);
    first
        .send(&Request::Node(NodeRequest::Spawn(task("sink", "cat", &[]))))
        .await;
    assert!(matches!(
        first.reply().await,
        DaemonMessage::Response(Response::Node(_))
    ));

    // The request claims the key, then waits for output the pane never prints.
    let waiting = Request::Node(NodeRequest::Wait {
        name: "sink".to_string(),
        condition: never(),
        timeout_ms: Some(60_000),
    });
    first
        .send(&Request::Idempotent {
            key: "k".to_string(),
            request: Box::new(waiting),
        })
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Cancelling the handler drops the claim unfinished, which releases the key.
    first.handler.abort();

    let mut repeat = raw_conn(&state);
    let quick = Request::Node(NodeRequest::Wait {
        name: "sink".to_string(),
        condition: never(),
        timeout_ms: Some(50),
    });
    repeat
        .send(&Request::Idempotent {
            key: "k".to_string(),
            request: Box::new(quick),
        })
        .await;
    let DaemonMessage::Response(Response::WaitResult(outcome)) = repeat.reply().await else {
        panic!("the released key runs the repeat");
    };
    assert!(!outcome.satisfied);
}
