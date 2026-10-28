use super::support::{connect, start, task};
use mpx::layout::Direction;
use mpx::protocol::{LayoutPreset, NewWindowOpts, NodeState, Side, SplitOpts};

fn split_of(target: &str, name: &str, focus: bool, percent: Option<u8>) -> SplitOpts {
    SplitOpts {
        target: Some(target.into()),
        spawn: task(name, "cat", &[]),
        direction: Direction::Horizontal,
        percent,
        focus,
    }
}

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

#[tokio::test]
async fn focus_last_returns_to_the_previous_pane() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client
        .split(split_of("left", "right", true, None))
        .await
        .unwrap();
    client.focus_last().await.unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(focused_pane(&layout).as_deref(), Some("left"));
    client.kill("left").await.unwrap();
    client.kill("right").await.unwrap();
}

#[tokio::test]
async fn split_percent_gives_the_new_pane_that_share() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("main-p", "cat", &[])).await.unwrap();
    client
        .split(split_of("main-p", "small", false, Some(25)))
        .await
        .unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.iter().find(|p| p.0 == "small").unwrap().1.width, 20);
    assert_eq!(
        focused_pane(&layout).as_deref(),
        Some("main-p"),
        "focus false"
    );
    client.kill("main-p").await.unwrap();
    client.kill("small").await.unwrap();
}

#[tokio::test]
async fn new_window_without_focus_keeps_the_active_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("first-win", "cat", &[])).await.unwrap();
    client
        .new_window(window_opts("bg", "bg-node", false))
        .await
        .unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 2);
    assert!(tabs[0].is_active);
    client.kill("first-win").await.unwrap();
    client.kill("bg-node").await.unwrap();
}

#[tokio::test]
async fn last_window_toggles_between_windows() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("w0-pane", "cat", &[])).await.unwrap();
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
    client.kill("w0-pane").await.unwrap();
    client.kill("w1-pane").await.unwrap();
}

#[tokio::test]
async fn rename_is_sticky_against_automatic_naming() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("named-pane", "cat", &[])).await.unwrap();
    client.rename_window(None, "work").await.unwrap();
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs[0].name, "work");
    client.kill("named-pane").await.unwrap();
}

#[tokio::test]
async fn window_is_named_after_its_agent_harness() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(window_opts("shell-win", "agent-pane", true))
        .await
        .unwrap();
    let (_, _, before) = client.layout(80, 24).await.unwrap();
    assert_eq!(before[0].name, "shell-win");

    client
        .agent_report(
            "agent-pane",
            mpx::client::AgentReportArgs {
                harness: Some("claude".into()),
                state: Some(mpx::protocol::AgentState::Working),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (_, _, after) = client.layout(80, 24).await.unwrap();
    assert_eq!(after[0].name, "claude");
    client.kill("agent-pane").await.unwrap();
}

#[tokio::test]
async fn kill_window_removes_all_its_panes() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("keep-win", "cat", &[])).await.unwrap();
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
    client.kill("keep-win").await.unwrap();
}

#[tokio::test]
async fn swap_pane_exchanges_positions() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("sw-a", "cat", &[])).await.unwrap();
    client
        .split(split_of("sw-a", "sw-b", false, None))
        .await
        .unwrap();
    client.swap_pane("sw-a", "sw-b").await.unwrap();
    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    let left = layout.iter().min_by_key(|p| p.1.x).unwrap();
    assert_eq!(left.0, "sw-b");
    client.kill("sw-a").await.unwrap();
    client.kill("sw-b").await.unwrap();
}

#[tokio::test]
async fn break_pane_moves_it_into_a_new_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("bk-a", "cat", &[])).await.unwrap();
    client
        .split(split_of("bk-a", "bk-b", true, None))
        .await
        .unwrap();
    client.break_pane("bk-b").await.unwrap();
    let (layout, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(layout.len(), 1);
    assert_eq!(layout[0].0, "bk-b");
    client.kill("bk-a").await.unwrap();
    client.kill("bk-b").await.unwrap();
}

#[tokio::test]
async fn even_horizontal_preset_equalises_widths() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("ev-a", "cat", &[])).await.unwrap();
    client
        .split(split_of("ev-a", "ev-b", false, Some(10)))
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
    client.kill("ev-a").await.unwrap();
    client.kill("ev-b").await.unwrap();
}

#[tokio::test]
async fn resize_split_moves_the_border() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("rs-a", "cat", &[])).await.unwrap();
    client
        .split(split_of("rs-a", "rs-b", false, None))
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
    client.kill("rs-a").await.unwrap();
    client.kill("rs-b").await.unwrap();
}

#[tokio::test]
async fn window_navigation_and_select_window_report_bounds() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("nav-a", "cat", &[])).await.unwrap();
    client
        .new_window(window_opts("nav-b", "nav-b-pane", true))
        .await
        .unwrap();
    client.next_window().await.unwrap();
    client.prev_window().await.unwrap();
    client.select_window(0).await.unwrap();
    assert!(client.select_window(9).await.is_err());
    client.kill("nav-a").await.unwrap();
    client.kill("nav-b-pane").await.unwrap();
}
