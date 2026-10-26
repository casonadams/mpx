use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::time::sleep;

use mpx::client::Client;
use mpx::daemon::{handle_client, DaemonState};
use mpx::ipc::IpcListener;
use mpx::protocol::StreamEvent;

async fn start_test_daemon() -> (String, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let sock_path = dir.path().join("test.sock").to_string_lossy().into_owned();

    #[cfg(unix)]
    let listener = IpcListener::bind(&sock_path).unwrap();
    #[cfg(windows)]
    let mut listener = IpcListener::bind(&sock_path).unwrap();

    let state = DaemonState::new();

    tokio::spawn(async move {
        loop {
            if let Ok(stream) = listener.accept().await {
                let s = Arc::clone(&state);
                tokio::spawn(async move {
                    handle_client(s, stream).await;
                });
            }
        }
    });

    // Give listener time to bind
    sleep(Duration::from_millis(50)).await;

    (sock_path, dir)
}

#[tokio::test]
async fn test_daemon_ping() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");
    client.ping().await.expect("Ping failed");
}

#[tokio::test]
async fn test_task_spawn_and_logs() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    let info = client
        .spawn(
            "echo-task".into(),
            "echo".into(),
            vec!["hello".into(), "world".into()],
            false,
            None,
        )
        .await
        .expect("Spawn failed");

    assert_eq!(info.name, "echo-task");
    assert!(!info.pty);

    let code = client.wait("echo-task").await.expect("Wait failed");
    assert_eq!(code, Some(0));

    let logs = client.logs("echo-task", None).await.expect("Logs failed");
    assert!(logs.contains("hello world"), "Logs did not contain 'hello world': {}", logs);
}

#[tokio::test]
async fn test_task_send_input() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    // Spawn cat to test stdin piping
    client
        .spawn("cat-task".into(), "cat".into(), vec![], false, None)
        .await
        .expect("Spawn cat failed");

    sleep(Duration::from_millis(100)).await;

    client
        .send_input("cat-task", "piped_input_data\n")
        .await
        .expect("Send input failed");

    sleep(Duration::from_millis(150)).await;

    let logs = client.logs("cat-task", None).await.expect("Logs failed");
    assert!(
        logs.contains("piped_input_data"),
        "Buffer did not capture stdin response: {}",
        logs
    );

    client.kill("cat-task").await.expect("Kill failed");
}

#[tokio::test]
async fn test_node_listing() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    client
        .spawn("task-1".into(), "echo".into(), vec!["1".into()], false, None)
        .await
        .unwrap();

    client
        .spawn("task-2".into(), "echo".into(), vec!["2".into()], false, None)
        .await
        .unwrap();

    let list = client.list().await.unwrap();
    assert_eq!(list.len(), 2);
    let names: Vec<String> = list.into_iter().map(|n| n.name).collect();
    assert!(names.contains(&"task-1".to_string()));
    assert!(names.contains(&"task-2".to_string()));
}

#[tokio::test]
async fn test_pty_spawn() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    let info = client
        .spawn(
            "pty-task".into(),
            "echo".into(),
            vec!["pty_output".into()],
            true,
            None,
        )
        .await
        .expect("PTY spawn failed");

    assert!(info.pty);

    let code = client.wait("pty-task").await.expect("Wait failed");
    assert_eq!(code, Some(0));

    let logs = client.logs("pty-task", None).await.expect("Logs failed");
    assert!(logs.contains("pty_output"), "PTY log output missing: {}", logs);
}

#[tokio::test]
async fn test_stream_subscription() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client_sub = Client::connect(&endpoint).await.expect("Failed to connect");
    let mut client_spawn = Client::connect(&endpoint).await.expect("Failed to connect");

    tokio::spawn(async move {
        sleep(Duration::from_millis(100)).await;
        client_spawn
            .spawn(
                "streamer".into(),
                "echo".into(),
                vec!["streaming_event_token".into()],
                false,
                None,
            )
            .await
            .unwrap();
    });

    let mut captured = false;
    let _ = client_sub
        .stream_logs(Some("streamer".to_string()), |event| match event {
            StreamEvent::Output { data, .. } => {
                if data.contains("streaming_event_token") {
                    captured = true;
                }
                true
            }
            StreamEvent::Exit { .. } => false,
            _ => true,
        })
        .await;

    assert!(captured, "Did not receive streamed token over socket subscription");
}

#[tokio::test]
async fn test_screen_snapshot() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    client
        .spawn(
            "screen-node".into(),
            "echo".into(),
            vec!["virtual_terminal_screen_text".into()],
            false,
            None,
        )
        .await
        .unwrap();

    let _ = client.wait("screen-node").await;
    let screen = client.screen("screen-node").await.expect("Screen failed");
    assert!(
        screen.contains("virtual_terminal_screen_text"),
        "Screen buffer missing text: {}",
        screen
    );
}

#[tokio::test]
async fn test_split_and_layout() {
    use mpx::layout::Direction;

    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    client
        .spawn("pane-left".into(), "echo".into(), vec!["left".into()], false, None)
        .await
        .unwrap();

    client
        .split(
            Some("pane-left".into()),
            "pane-right".into(),
            "echo".into(),
            vec!["right".into()],
            false,
            Direction::Horizontal,
        )
        .await
        .unwrap();

    let layout = client.layout(80, 24).await.expect("Layout failed");
    assert_eq!(layout.len(), 2);
    let left = layout.iter().find(|(n, ..)| n == "pane-left").unwrap();
    let right = layout.iter().find(|(n, ..)| n == "pane-right").unwrap();

    assert_eq!(left.1.width, 40);
    assert_eq!(right.1.width, 40);
    assert_eq!(left.1.height, 24);
    assert_eq!(right.1.height, 24);
}

#[tokio::test]
async fn test_virtual_pipe_e2e() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    client.create_pipe("agent_bus", None).await.expect("Create pipe failed");
    client.write_pipe("agent_bus", "message from agent 1\n").await.expect("Write pipe failed");
    client.write_pipe("agent_bus", "message from agent 2\n").await.expect("Write pipe failed");

    let content = client.read_pipe("agent_bus").await.expect("Read pipe failed");
    assert!(content.contains("message from agent 1"));
    assert!(content.contains("message from agent 2"));

    let list = client.list_pipes().await.expect("List pipes failed");
    assert!(list.contains(&"agent_bus".to_string()));
}
