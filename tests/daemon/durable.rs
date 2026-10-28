use std::path::Path;
use std::time::Duration;

use super::support::{connect, eventually, pty, start_on};
use mpx::client::PipeReadArgs;
use mpx::config::Config;
use mpx::layout::Direction;
use mpx::protocol::{NewWindowOpts, SplitOpts};

fn split_of(target: &str, name: &str) -> SplitOpts {
    SplitOpts {
        target: Some(target.to_string()),
        spawn: pty(name, "cat", &[]),
        direction: Direction::Horizontal,
        percent: None,
        focus: true,
    }
}

/// Everything a client sees about the layout, in a comparable form.
async fn layout_snapshot(client: &mut mpx::client::Client) -> String {
    let (rects, dividers, tabs) = client.layout(80, 24).await.unwrap();
    format!("{rects:?}{dividers:?}{tabs:?}")
}

fn snapshot_names(state: &Path) -> Option<String> {
    std::fs::read_to_string(state.join("snapshot.json")).ok()
}

#[tokio::test]
async fn pipe_messages_and_cursors_survive_a_restart_on_the_same_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    {
        let daemon = start_on(&state, Config::default()).await;
        let mut client = connect(&daemon.endpoint).await;
        client.create_pipe("bus", None).await.unwrap();
        for message in ["a", "b", "c"] {
            client.write_pipe("bus", message).await.unwrap();
        }
        let (read, _) = client
            .read_pipe(
                "bus",
                PipeReadArgs {
                    consumer: Some("reader".to_string()),
                    limit: Some(2),
                    ..PipeReadArgs::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(read.len(), 2);
    }

    let daemon = start_on(&state, Config::default()).await;
    let mut client = connect(&daemon.endpoint).await;
    let (read, _) = client
        .read_pipe(
            "bus",
            PipeReadArgs {
                consumer: Some("reader".to_string()),
                ..PipeReadArgs::default()
            },
        )
        .await
        .unwrap();
    let data: Vec<&str> = read.iter().map(|m| m.data.as_str()).collect();
    assert_eq!(data, ["c"], "resumes from the saved cursor");
    assert_eq!(client.write_pipe("bus", "d").await.unwrap(), 4);
}

#[tokio::test]
async fn crash_restore_recreates_panes_layout_focus_and_window_names() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let before = {
        let daemon = start_on(&state, Config::default()).await;
        let mut client = connect(&daemon.endpoint).await;
        client.spawn(pty("editor", "cat", &[])).await.unwrap();
        client.split(split_of("editor", "shell")).await.unwrap();
        client
            .new_window(NewWindowOpts {
                name: None,
                spawn: pty("logs", "cat", &[]),
                focus: true,
            })
            .await
            .unwrap();
        client.rename_window(None, "work").await.unwrap();
        client.select_window(0).await.unwrap();
        client.focus("shell").await.unwrap();
        // Let the debounced snapshot capture the final state.
        tokio::time::sleep(Duration::from_millis(600)).await;
        let state_ref = state.clone();
        eventually(Duration::from_secs(2), || {
            snapshot_names(&state_ref).filter(|s| s.contains("logs") && s.contains("shell"))
        })
        .await;
        layout_snapshot(&mut client).await
    };

    let daemon = start_on(&state, Config::default()).await;
    let mut client = connect(&daemon.endpoint).await;
    assert!(client.info().await.unwrap().restored_ms.is_some());
    assert_eq!(layout_snapshot(&mut client).await, before);
    let nodes: Vec<String> = client
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert!(nodes.iter().any(|n| n == "editor") && nodes.iter().any(|n| n == "logs"));
}

#[tokio::test]
async fn clean_shutdown_is_not_auto_restored_but_restore_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    {
        let daemon = start_on(&state, Config::default()).await;
        let mut client = connect(&daemon.endpoint).await;
        client.spawn(pty("keep", "cat", &[])).await.unwrap();
        client.shutdown().await.unwrap();
    }

    let daemon = start_on(&state, Config::default()).await;
    let mut client = connect(&daemon.endpoint).await;
    assert!(
        client.list().await.unwrap().is_empty(),
        "no auto-restore after clean shutdown"
    );
    client.restore().await.unwrap();
    let names: Vec<String> = client
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert_eq!(names, ["keep"]);

    let err = client.restore().await.unwrap_err();
    assert!(err.contains("already has panes"), "error: {err}");
}

#[tokio::test]
async fn restore_without_a_snapshot_is_an_error() {
    let daemon = super::support::start().await;
    let mut client = connect(&daemon.endpoint).await;
    let err = client.restore().await.unwrap_err();
    assert!(err.contains("snapshot"), "error: {err}");
}
