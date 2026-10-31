use super::support::{connect, split_right, split_with, start, started_with, task};
use mpx::client::Client;
use mpx::protocol::{LayoutPreset, NewWindowOpts, NodeState, Side};

fn window_opts(label: &str, name: &str, focus: bool) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.into()),
        spawn: task(name, "cat", &[]),
        focus,
    }
}

fn focused_pane(layout: &[(String, mpx::layout::Rect, bool)]) -> Option<String> {
    layout.iter().find(|p| p.2).map(|p| p.0.clone())
}

async fn kill_all(client: &mut Client, names: &[&str]) {
    for name in names {
        client.kill(name).await.unwrap();
    }
}

#[tokio::test]
async fn focus_last_returns_to_the_previous_pane() {
    let (_daemon, mut client) = started_with(task("left", "cat", &[])).await;
    client.split(split_right("left", "right")).await.unwrap();
    client.focus_last().await.unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(focused_pane(&layout).as_deref(), Some("left"));
    kill_all(&mut client, &["left", "right"]).await;
}

#[tokio::test]
async fn split_percent_gives_the_new_pane_that_share() {
    let (_daemon, mut client) = started_with(task("main-p", "cat", &[])).await;
    client
        .split(split_with("main-p", "small", false, Some(25)))
        .await
        .unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.iter().find(|p| p.0 == "small").unwrap().1.width, 20);
    assert_eq!(
        focused_pane(&layout).as_deref(),
        Some("main-p"),
        "focus false"
    );
    kill_all(&mut client, &["main-p", "small"]).await;
}

#[tokio::test]
async fn new_window_without_focus_keeps_the_active_window() {
    let (_daemon, mut client) = started_with(task("first-win", "cat", &[])).await;
    client
        .new_window(window_opts("bg", "bg-node", false))
        .await
        .unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 2);
    assert!(tabs[0].is_active);
    kill_all(&mut client, &["first-win", "bg-node"]).await;
}

#[tokio::test]
async fn last_window_toggles_between_windows() {
    let (_daemon, mut client) = started_with(task("w0-pane", "cat", &[])).await;
    client
        .new_window(window_opts("w1", "w1-pane", true))
        .await
        .unwrap();
    client.last_window().await.unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert!(tabs[0].is_active);
    client.last_window().await.unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert!(tabs[1].is_active);
    kill_all(&mut client, &["w0-pane", "w1-pane"]).await;
}

#[tokio::test]
async fn rename_is_sticky_against_automatic_naming() {
    let (_daemon, mut client) = started_with(task("named-pane", "cat", &[])).await;
    client.rename_window(None, "work").await.unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs[0].name, "work");
    kill_all(&mut client, &["named-pane"]).await;
}

#[tokio::test]
async fn unnamed_window_is_named_after_its_command() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(NewWindowOpts {
            name: None,
            ..window_opts("", "cat-pane", true)
        })
        .await
        .unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs[0].name, "cat");
    kill_all(&mut client, &["cat-pane"]).await;
}

#[tokio::test]
async fn a_name_given_at_creation_is_kept_against_automatic_naming() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(window_opts("shell-win", "cat-pane", true))
        .await
        .unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs[0].name, "shell-win");
    kill_all(&mut client, &["cat-pane"]).await;
}

#[tokio::test]
async fn kill_window_removes_all_its_panes() {
    let (_daemon, mut client) = started_with(task("keep-win", "cat", &[])).await;
    client
        .new_window(window_opts("doomed", "doomed-pane", true))
        .await
        .unwrap();
    client.kill_window(None).await.unwrap();
    assert_eq!(
        client.status("doomed-pane").await.unwrap().state,
        NodeState::Exited
    );
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 1);
    kill_all(&mut client, &["keep-win"]).await;
}

#[tokio::test]
async fn swap_pane_exchanges_positions() {
    let (_daemon, mut client) = started_with(task("sw-a", "cat", &[])).await;
    client
        .split(split_with("sw-a", "sw-b", false, None))
        .await
        .unwrap();
    client.swap_pane("sw-a", "sw-b").await.unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    let left = layout.iter().min_by_key(|p| p.1.x).unwrap();
    assert_eq!(left.0, "sw-b");
    kill_all(&mut client, &["sw-a", "sw-b"]).await;
}

#[tokio::test]
async fn mark_pane_and_swap_with_marked_pane() {
    let (_daemon, mut client) = started_with(task("mk-a", "cat", &[])).await;
    client.split(split_right("mk-a", "mk-b")).await.unwrap();

    // Mark mk-a (which is not focused since mk-b was focused on split)
    client.mark_pane(Some("mk-a"), false).await.unwrap();

    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert!(tabs[0].has_marked);
    assert_eq!(tabs[0].marked_pane.as_deref(), Some("mk-a"));

    // Check node info has marked: true for mk-a
    let nodes = client.list().await.unwrap();
    let node_a = nodes.iter().find(|n| n.name == "mk-a").unwrap();
    assert_eq!(node_a.marked, Some(true));
    let node_b = nodes.iter().find(|n| n.name == "mk-b").unwrap();
    assert_eq!(node_b.marked, Some(false));

    // Swapping without source swaps active (mk-b) with marked (mk-a)
    client.swap_pane_targets(None, None).await.unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    let left = layout.iter().min_by_key(|p| p.1.x).unwrap();
    assert_eq!(left.0, "mk-b");

    // Clear mark
    client.mark_pane(None, true).await.unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert!(!tabs[0].has_marked);

    kill_all(&mut client, &["mk-a", "mk-b"]).await;
}

#[tokio::test]
async fn break_pane_moves_it_into_a_new_window() {
    let (_daemon, mut client) = started_with(task("bk-a", "cat", &[])).await;
    client.split(split_right("bk-a", "bk-b")).await.unwrap();
    client.break_pane("bk-b").await.unwrap();
    let (layout, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(layout.len(), 1);
    assert_eq!(layout[0].0, "bk-b");
    kill_all(&mut client, &["bk-a", "bk-b"]).await;
}

#[tokio::test]
async fn even_horizontal_preset_equalises_widths() {
    let (_daemon, mut client) = started_with(task("ev-a", "cat", &[])).await;
    client
        .split(split_with("ev-a", "ev-b", false, Some(10)))
        .await
        .unwrap();
    client
        .select_layout(Some(LayoutPreset::EvenHorizontal))
        .await
        .unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    let a = layout.iter().find(|p| p.0 == "ev-a").unwrap().1.width;
    let b = layout.iter().find(|p| p.0 == "ev-b").unwrap().1.width;
    assert!(a.abs_diff(b) <= 1, "widths {a} and {b}");
    kill_all(&mut client, &["ev-a", "ev-b"]).await;
}

#[tokio::test]
async fn resize_split_moves_the_border() {
    let (_daemon, mut client) = started_with(task("rs-a", "cat", &[])).await;
    client
        .split(split_with("rs-a", "rs-b", false, None))
        .await
        .unwrap();
    client
        .resize_split("rs-a", Side::Right, 10, (80, 24))
        .await
        .unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.iter().find(|p| p.0 == "rs-a").unwrap().1.width, 50);
    assert!(
        client
            .resize_split("rs-a", Side::Up, 2, (80, 24))
            .await
            .is_err()
    );
    kill_all(&mut client, &["rs-a", "rs-b"]).await;
}

#[tokio::test]
async fn window_navigation_and_select_window_report_bounds() {
    let (_daemon, mut client) = started_with(task("nav-a", "cat", &[])).await;
    client
        .new_window(window_opts("nav-b", "nav-b-pane", true))
        .await
        .unwrap();
    client.next_window().await.unwrap();
    client.prev_window().await.unwrap();
    client.select_window(0).await.unwrap();
    assert!(client.select_window(9).await.is_err());
    kill_all(&mut client, &["nav-a", "nav-b-pane"]).await;
}
