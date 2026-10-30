//! Window rotation and in-place respawn through the daemon client.
use std::time::Duration;

use mpx::client::Client;
use mpx::layout::{Direction, Rect};
use mpx::protocol::{NodeInfo, SpawnOpts, SplitOpts};

use super::support::{connect, exit_code, logs_until, start, task};

fn names(layout: &[(String, Rect, bool)]) -> Vec<String> {
    layout.iter().map(|p| p.0.clone()).collect()
}

fn split_right(target: &str, name: &str) -> SplitOpts {
    SplitOpts {
        target: Some(target.into()),
        spawn: task(name, "cat", &[]),
        direction: Direction::Horizontal,
        percent: None,
        focus: true,
    }
}

async fn node(client: &mut Client, name: &str) -> NodeInfo {
    client
        .list()
        .await
        .expect("list")
        .into_iter()
        .find(|n| n.name == name)
        .expect("node exists")
}

#[tokio::test]
async fn rotate_window_moves_names_through_the_slots_and_focus_stays_on_its_pane() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("a", "cat", &[])).await.unwrap();
    client.split(split_right("a", "b")).await.unwrap();
    client.split(split_right("b", "c")).await.unwrap();

    client.rotate_window(false).await.unwrap();
    let (layout, _, _) = client.layout(90, 10).await.unwrap();
    assert_eq!(names(&layout), ["b", "c", "a"]);
    assert!(layout.iter().any(|p| p.0 == "c" && p.2), "focus follows c");

    client.rotate_window(true).await.unwrap();
    let (layout, _, _) = client.layout(90, 10).await.unwrap();
    assert_eq!(names(&layout), ["a", "b", "c"]);

    for name in ["a", "b", "c"] {
        client.kill(name).await.unwrap();
    }
}

#[tokio::test]
async fn rotate_window_without_a_window_is_an_error() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    assert!(client.rotate_window(false).await.is_err());
}

#[tokio::test]
async fn respawn_refuses_a_running_pane_unless_it_is_killed() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("p", "cat", &[])).await.unwrap();

    let err = client.respawn("p", false, None, None).await.unwrap_err();
    assert!(err.contains("still active"), "{err}");

    client.kill("p").await.unwrap();
}

#[tokio::test]
async fn respawn_restarts_an_exited_pane_in_its_slot_and_keeps_its_id() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let exits = SpawnOpts {
        remain_on_exit: Some(true),
        ..task("p", "sh", &["-c", "exit 3"])
    };
    client.spawn(exits).await.unwrap();
    client.split(split_right("p", "q")).await.unwrap();
    assert_eq!(exit_code(&mut client, "p").await, Some(3));
    let id = node(&mut client, "p").await.id;

    let command = vec!["sh".to_string(), "-c".into(), "echo again; exec cat".into()];
    client
        .respawn("p", false, None, Some(command))
        .await
        .unwrap();
    logs_until(&mut client, "p", "again").await;

    let respawned = node(&mut client, "p").await;
    assert_eq!(respawned.id, id);
    assert!(respawned.is_running());
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(names(&layout), ["p", "q"]);

    client.kill("p").await.unwrap();
    client.kill("q").await.unwrap();
}

#[tokio::test]
async fn respawn_with_kill_replaces_the_process_and_keeps_its_slot() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();

    client
        .respawn("left", true, None, Some(vec!["cat".into()]))
        .await
        .unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(names(&layout), ["left", "right"]);

    // The killed process exits after the respawn; its exit must not end the replacement.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(node(&mut client, "left").await.is_running());

    client.kill("left").await.unwrap();
    client.kill("right").await.unwrap();
}

#[tokio::test]
async fn respawn_of_an_unknown_pane_is_an_error() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    assert!(client.respawn("nope", true, None, None).await.is_err());
}
