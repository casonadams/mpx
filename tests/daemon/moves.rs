//! Moving panes between windows, swapping windows and respawning panes through the daemon client.
use std::time::Duration;

use mpx::client::Client;
use mpx::layout::Rect;
use mpx::protocol::{NewWindowOpts, NodeInfo, SpawnOpts};

use super::support::{connect, exit_code, logs_until, split_right, start, task};

fn names(layout: &[(String, Rect, bool)]) -> Vec<String> {
    layout.iter().map(|p| p.0.clone()).collect()
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

fn window(label: &str, pane: &str) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.to_string()),
        spawn: task(pane, "cat", &[]),
        focus: true,
    }
}

async fn tabs(client: &mut Client) -> Vec<(String, bool)> {
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    tabs.into_iter().map(|t| (t.name, t.is_active)).collect()
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

#[tokio::test]
async fn swap_window_exchanges_two_windows_and_the_active_one_stays_active() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for (label, pane) in [("a", "pa"), ("b", "pb"), ("c", "pc")] {
        client.new_window(window(label, pane)).await.unwrap();
    }
    client.select_window(0).await.unwrap();

    client.swap_window(0, 2).await.unwrap();
    assert_eq!(
        tabs(&mut client).await,
        [("c".into(), false), ("b".into(), false), ("a".into(), true)]
    );
    assert!(client.swap_window(0, 9).await.is_err());
}

#[tokio::test]
async fn join_pane_moves_a_pane_next_to_another_and_drops_the_emptied_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();

    client.join_pane("pb", "pa", true).await.unwrap();

    let (rects, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 1, "window b had only that pane");
    let mut names: Vec<_> = rects.iter().map(|r| r.0.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["pa", "pb"]);
    let (left, right) = (&rects[0].1, &rects[1].1);
    assert_eq!(left.height, right.height, "side by side share the height");

    assert!(client.join_pane("pa", "pa", true).await.is_err());
    assert!(client.join_pane("nope", "pa", true).await.is_err());
}

#[tokio::test]
async fn join_pane_stacked_shares_the_width() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();

    client.join_pane("pb", "pa", false).await.unwrap();

    let (rects, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(rects[0].1.width, rects[1].1.width);
}
