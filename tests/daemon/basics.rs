use std::time::Duration;

use super::support::{connect, exit_code, logs_until, pty, start, task};
use mpx::layout::Direction;
use mpx::protocol::{NewWindowOpts, SplitOpts, StreamEvent};

#[tokio::test]
async fn ping_and_info_describe_the_session() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.ping().await.unwrap();
    let info = client.info().await.unwrap();
    assert_eq!(info.name, "test");
    assert_eq!(info.endpoint, daemon.endpoint);
    assert_eq!(info.pid, std::process::id());
    assert_eq!(info.panes, 0);
}

#[tokio::test]
async fn task_output_is_captured_with_exit_code() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let info = client
        .spawn(task("echo-task", "echo", &["hello", "world"]))
        .await
        .unwrap();
    assert_eq!(info.name, "echo-task");
    assert!(!info.pty);

    assert_eq!(exit_code(&mut client, "echo-task").await, Some(0));
    let (logs, _) = client.logs("echo-task", None, false, None).await.unwrap();
    assert!(logs.contains("hello world"), "logs: {logs}");
}

#[tokio::test]
async fn pty_output_is_captured() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let info = client
        .spawn(pty("pty-task", "echo", &["pty_output"]))
        .await
        .unwrap();
    assert!(info.pty);
    assert_eq!(exit_code(&mut client, "pty-task").await, Some(0));
    let logs = logs_until(&mut client, "pty-task", "pty_output").await;
    assert!(logs.contains("pty_output"), "logs: {logs}");
}

#[tokio::test]
async fn interactive_pty_shell_runs_input() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("shell", "sh", &[])).await.unwrap();
    client
        .send_input("shell", "echo PTY_INTERACTIVE_OK\r\n")
        .await
        .unwrap();
    let logs = logs_until(&mut client, "shell", "PTY_INTERACTIVE_OK").await;
    assert!(logs.contains("PTY_INTERACTIVE_OK"), "logs: {logs}");
    client.kill("shell").await.unwrap();
}

#[tokio::test]
async fn stream_subscription_receives_output() {
    let daemon = start().await;
    let mut sub = connect(&daemon.endpoint).await;
    let mut spawner = connect(&daemon.endpoint).await;

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        spawner
            .spawn(task("streamer", "echo", &["streaming_event_token"]))
            .await
            .unwrap();
    });

    let mut captured = false;
    sub.stream_logs(Some("streamer".to_string()), None, |event| match event {
        StreamEvent::Output { data, .. } => {
            captured |= data.contains("streaming_event_token");
            true
        }
        StreamEvent::Exit { .. } => false,
        _ => true,
    })
    .await
    .unwrap();
    assert!(captured);
}

#[tokio::test]
async fn screen_snapshot_shows_program_output() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("screen-node", "echo", &["virtual_screen_text"]))
        .await
        .unwrap();
    exit_code(&mut client, "screen-node").await;
    let screen = client.screen("screen-node").await.unwrap();
    assert!(screen.contains("virtual_screen_text"), "screen: {screen}");
}

#[tokio::test]
async fn split_lays_out_two_panes_and_exit_collapses_the_layout() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("keep-alive", "cat", &[])).await.unwrap();
    client
        .split(SplitOpts {
            target: Some("keep-alive".into()),
            spawn: task("quick-exit", "sleep", &["0.5"]),
            direction: Direction::Horizontal,
            percent: None,
            focus: true,
        })
        .await
        .unwrap();

    let (layout, dividers, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.len(), 2);
    assert_eq!(dividers.len(), 1);
    assert_eq!(
        layout.iter().find(|p| p.0 == "keep-alive").unwrap().1.width,
        40
    );

    exit_code(&mut client, "quick-exit").await;
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.len(), 1, "exited pane is removed");
    assert_eq!(layout[0].1.width, 80);
    client.kill("keep-alive").await.unwrap();
}

#[tokio::test]
async fn resize_pane_is_accepted_for_pty_nodes() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("resizer", "cat", &[])).await.unwrap();
    client.resize_pane("resizer", 120, 40).await.unwrap();
    client.kill("resizer").await.unwrap();
}

#[tokio::test]
async fn new_window_reports_cwd_and_split_inherits_it() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let win = client
        .new_window(NewWindowOpts {
            name: Some("test-win".into()),
            spawn: mpx::protocol::SpawnOpts {
                name: "win-node".into(),
                command: "cat".into(),
                cwd: Some(dir.clone()),
                ..Default::default()
            },
            focus: true,
        })
        .await
        .unwrap();
    assert_eq!(client.get_cwd(&win.name).await.unwrap(), Some(dir.clone()));

    let split = client
        .split(SplitOpts {
            target: Some(win.name.clone()),
            spawn: mpx::protocol::SpawnOpts {
                name: "split-node".into(),
                command: "cat".into(),
                cwd: Some(dir.clone()),
                ..Default::default()
            },
            direction: Direction::Horizontal,
            percent: None,
            focus: true,
        })
        .await
        .unwrap();
    assert_eq!(client.get_cwd(&split.name).await.unwrap(), Some(dir));
    client.kill("win-node").await.unwrap();
    client.kill("split-node").await.unwrap();
}
