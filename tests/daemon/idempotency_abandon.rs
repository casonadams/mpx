use std::sync::Arc;
use std::time::Duration;

use mpx::client::{Client, PipeReadArgs};
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::IpcListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::support::{SESSION, connect};

struct Daemon {
    endpoint: String,
    connections: mpsc::UnboundedReceiver<JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

/// A daemon that reports each connection's handler task, so a test can tear one down mid-flight.
async fn start_tracked() -> Daemon {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = mpx::ipc::endpoint_in(dir.path(), SESSION);
    #[allow(unused_mut)]
    let mut listener = IpcListener::bind(&endpoint).expect("bind");
    let state = DaemonState::new(
        SESSION.to_string(),
        endpoint.clone(),
        Config::default(),
        dir.path().join("state"),
    )
    .await;
    let (tx, connections) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            let handler = tokio::spawn(handle_client(Arc::clone(&state), stream));
            if tx.send(handler).is_err() {
                break;
            }
        }
    });
    Daemon {
        endpoint,
        connections,
        _dir: dir,
    }
}

impl Daemon {
    /// Connects a client and returns it with its server-side handler task.
    async fn client(&mut self) -> (Client, JoinHandle<()>) {
        let client = connect(&self.endpoint).await;
        let handler = tokio::time::timeout(Duration::from_secs(2), self.connections.recv())
            .await
            .expect("connection accepted")
            .expect("accept loop running");
        (client, handler)
    }
}

#[tokio::test]
async fn abandoned_claim_lets_a_repeat_of_its_key_run_again() {
    let mut daemon = start_tracked().await;
    let (mut setup, _) = daemon.client().await;
    setup.create_pipe("gate", None).await.unwrap();

    let (mut victim, victim_handler) = daemon.client().await;
    let pending = tokio::spawn(async move {
        victim
            .idempotency_key("abandon-1")
            .read_pipe(
                "gate",
                PipeReadArgs {
                    wait_ms: Some(5_000),
                    ..PipeReadArgs::default()
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    victim_handler.abort();
    assert!(
        pending.await.unwrap().is_err(),
        "the torn-down connection gets no response"
    );

    setup.write_pipe("gate", "hi").await.unwrap();
    let (mut again, _) = daemon.client().await;
    let (messages, _) = tokio::time::timeout(
        Duration::from_secs(1),
        again
            .idempotency_key("abandon-1")
            .read_pipe("gate", PipeReadArgs::default()),
    )
    .await
    .expect("the repeat does not wait on the abandoned claim")
    .unwrap();
    let data: Vec<&str> = messages.iter().map(|m| m.data.as_str()).collect();
    assert_eq!(data, vec!["hi"]);
}

#[tokio::test]
async fn repeat_after_a_disconnected_client_gets_the_original_answer() {
    let mut daemon = start_tracked().await;
    let (mut setup, _) = daemon.client().await;
    setup.create_pipe("late", None).await.unwrap();

    let (mut first, _) = daemon.client().await;
    let pending = tokio::spawn(async move {
        first
            .idempotency_key("keep-1")
            .read_pipe(
                "late",
                PipeReadArgs {
                    wait_ms: Some(200),
                    ..PipeReadArgs::default()
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    pending.abort();
    tokio::time::sleep(Duration::from_millis(400)).await;

    setup.write_pipe("late", "after").await.unwrap();
    let (mut repeat, _) = daemon.client().await;
    let (messages, _) = tokio::time::timeout(
        Duration::from_secs(1),
        repeat
            .idempotency_key("keep-1")
            .read_pipe("late", PipeReadArgs::default()),
    )
    .await
    .expect("the repeat is answered from the cache")
    .unwrap();
    assert!(messages.is_empty(), "the original answer, not a second run");
}
