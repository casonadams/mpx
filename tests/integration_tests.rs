use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::time::sleep;

use mpx::client::Client;
use mpx::daemon::{handle_client, DaemonState};
use mpx::ipc::IpcListener;
use mpx::layout::Direction;
use mpx::protocol::{SplitOpts, StreamEvent};

async fn start_test_daemon() -> (String, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    #[cfg(unix)]
    let sock_path = dir.path().join("test.sock").to_string_lossy().into_owned();
    #[cfg(windows)]
    let sock_path = format!(
        r"\\.\pipe\mpx-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

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
        .spawn("task-1".into(), "cat".into(), vec![], false, None)
        .await
        .unwrap();

    client
        .spawn("task-2".into(), "cat".into(), vec![], false, None)
        .await
        .unwrap();

    let list = client.list().await.unwrap();
    assert_eq!(list.len(), 2);
    let names: Vec<String> = list.into_iter().map(|n| n.name).collect();
    assert!(names.contains(&"task-1".to_string()));
    assert!(names.contains(&"task-2".to_string()));

    client.kill("task-1").await.unwrap();
    client.kill("task-2").await.unwrap();
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
async fn test_interactive_pty_shell() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    let shell = if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    };
    client
        .spawn("shell-test".into(), shell, vec![], true, None)
        .await
        .expect("Spawn shell failed");

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    client
        .send_input("shell-test", "echo PTY_INTERACTIVE_OK\r\n")
        .await
        .expect("Send input failed");

    let mut found = false;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(logs) = client.logs("shell-test", None).await {
            if logs.contains("PTY_INTERACTIVE_OK") {
                found = true;
                break;
            }
        }
    }
    assert!(found, "PTY did not execute interactive input");
    client.kill("shell-test").await.unwrap();
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
        .spawn("pane-left".into(), "cat".into(), vec![], false, None)
        .await
        .unwrap();

    client
        .split(SplitOpts {
            target: Some("pane-left".into()),
            name: "pane-right".into(),
            command: "cat".into(),
            args: vec![],
            pty: false,
            direction: Direction::Horizontal,
            cwd: None,
            focus: true,
        })
        .await
        .unwrap();

    let (layout, dividers, _tabs) = client.layout(80, 24).await.expect("Layout failed");
    assert_eq!(layout.len(), 2);
    assert_eq!(dividers.len(), 1);
    let left = layout.iter().find(|(n, ..)| n == "pane-left").unwrap();
    let right = layout.iter().find(|(n, ..)| n == "pane-right").unwrap();

    assert_eq!(left.1.width, 40);
    assert_eq!(right.1.width, 39);
    assert_eq!(left.1.height, 24);
    assert_eq!(right.1.height, 24);

    client.kill("pane-left").await.unwrap();
    client.kill("pane-right").await.unwrap();
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

#[tokio::test]
async fn test_pane_auto_removed_on_exit() {
    use mpx::layout::Direction;

    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    // Spawn long running node 1
    client
        .spawn("keep-alive".into(), "cat".into(), vec![], false, None)
        .await
        .unwrap();

    // Split with node 2 that exits quickly
    client
        .split(SplitOpts {
            target: Some("keep-alive".into()),
            name: "quick-exit".into(),
            command: "echo".into(),
            args: vec!["bye".into()],
            pty: false,
            direction: Direction::Horizontal,
            cwd: None,
            focus: true,
        })
        .await
        .unwrap();

    // Wait for node 2 to finish
    let _ = client.wait("quick-exit").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Layout must automatically have purged "quick-exit" and reclaimed the full width!
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.len(), 1, "Layout should have collapsed back to 1 pane");
    assert_eq!(layout[0].0, "keep-alive");
    assert_eq!(layout[0].1.width, 80);

    client.kill("keep-alive").await.unwrap();
}

#[tokio::test]
async fn test_resize_pane_e2e() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    client
        .spawn("resizer".into(), "cat".into(), vec![], true, None)
        .await
        .unwrap();

    client.resize_pane("resizer", 120, 40).await.expect("Resize pane failed");

    client.kill("resizer").await.unwrap();
}

#[tokio::test]
async fn test_cwd_propagation_e2e() {
    use mpx::layout::Direction;
    use mpx::protocol::NewWindowOpts;

    let (endpoint, dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    let custom_dir = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();

    let win_info = client
        .new_window(NewWindowOpts {
            name: Some("test-win".into()),
            command: "cat".into(),
            args: vec![],
            pty: false,
            cwd: Some(custom_dir.clone()),
        })
        .await
        .unwrap();

    let fetched_cwd = client.get_cwd(&win_info.name).await.unwrap();
    assert_eq!(fetched_cwd, Some(custom_dir.clone()));

    let split_info = client
        .split(SplitOpts {
            target: Some(win_info.name.clone()),
            name: "test-split".into(),
            command: "cat".into(),
            args: vec![],
            pty: false,
            direction: Direction::Horizontal,
            cwd: Some(custom_dir.clone()),
            focus: true,
        })
        .await
        .unwrap();

    let split_cwd = client.get_cwd(&split_info.name).await.unwrap();
    assert_eq!(split_cwd, Some(custom_dir));

    client.kill(&win_info.name).await.unwrap();
    client.kill(&split_info.name).await.unwrap();
}

#[tokio::test]
async fn test_split_detach_focus_and_status() {
    let (endpoint, _dir) = start_test_daemon().await;
    let mut client = Client::connect(&endpoint).await.expect("Failed to connect");

    // Spawn primary pane
    client
        .spawn("pane-main".into(), "cat".into(), vec![], false, None)
        .await
        .unwrap();

    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout[0].0, "pane-main");
    assert!(layout[0].2, "pane-main should initially be focused");

    // Split secondary pane in background without stealing focus (focus: false)
    client
        .split(SplitOpts {
            target: Some("pane-main".into()),
            name: "pane-bg".into(),
            command: "cat".into(),
            args: vec![],
            pty: false,
            direction: Direction::Horizontal,
            cwd: None,
            focus: false,
        })
        .await
        .unwrap();

    // Verify layout: pane-main MUST STILL be focused!
    let (layout2, _, _) = client.layout(80, 24).await.unwrap();
    let main = layout2.iter().find(|(n, ..)| n == "pane-main").unwrap();
    let bg = layout2.iter().find(|(n, ..)| n == "pane-bg").unwrap();
    assert!(main.2, "pane-main must keep focus when split has focus: false");
    assert!(!bg.2, "pane-bg must NOT have stolen focus");

    // Test client.status() on running node
    let status_bg = client.status("pane-bg").await.unwrap();
    assert_eq!(status_bg.name, "pane-bg");
    assert_eq!(status_bg.status, "running");

    // Explicitly focus pane-bg via client.focus()
    client.focus("pane-bg").await.unwrap();
    let (layout3, _, _) = client.layout(80, 24).await.unwrap();
    let main3 = layout3.iter().find(|(n, ..)| n == "pane-main").unwrap();
    let bg3 = layout3.iter().find(|(n, ..)| n == "pane-bg").unwrap();
    assert!(!main3.2, "pane-main should no longer be focused");
    assert!(bg3.2, "pane-bg should now be focused");

    // Kill pane-bg and verify exited status
    client.kill("pane-bg").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let list = client.list().await.unwrap();
    let bg_entry = list.iter().find(|n| n.name == "pane-bg");
    assert!(bg_entry.is_some(), "Exited node should be preserved in list history");

    let status_exited = client.status("pane-bg").await.unwrap();
    assert!(status_exited.status.starts_with("exited"));

    client.kill("pane-main").await.unwrap();
}

#[tokio::test]
async fn test_task_node_direct_and_clipboard() {
    use mpx::node::task::TaskNode;
    use mpx::node::Node;
    use mpx::tui::render::copy_to_clipboard;
    use tokio::sync::broadcast;

    let (event_tx, _) = broadcast::channel(16);
    let task = TaskNode::spawn(
        "test-task-direct".into(),
        "cat".into(),
        &[],
        None,
        event_tx,
    )
    .unwrap();

    assert!(task.is_running());
    task.send_input(b"hello direct\n").unwrap();
    task.kill().unwrap();

    copy_to_clipboard("test clipboard");
}
