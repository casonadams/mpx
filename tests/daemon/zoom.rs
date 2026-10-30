//! Zoom is the daemon's per-window flag: toggled by requests, followed by every terminal through
//! the layout it reports, and cleared by the changes that move focus or rearrange the layout.
use super::support::{connect, start, task};
use mpx::client::Client;
use mpx::layout::{Direction, WindowTab};
use mpx::protocol::{NewWindowOpts, SplitOpts};

fn split_right(target: &str, name: &str) -> SplitOpts {
    SplitOpts {
        target: Some(target.into()),
        spawn: task(name, "cat", &[]),
        direction: Direction::Horizontal,
        percent: None,
        focus: true,
    }
}

async fn tabs(client: &mut Client) -> Vec<WindowTab> {
    client.layout(80, 24).await.unwrap().2
}

/// Whether the active window's flag is set.
async fn zoomed(client: &mut Client) -> bool {
    tabs(client).await.iter().any(|t| t.is_active && t.zoomed)
}

async fn focused(client: &mut Client) -> Option<String> {
    let (rects, _, _) = client.layout(80, 24).await.unwrap();
    rects.into_iter().find(|p| p.2).map(|p| p.0)
}

#[tokio::test]
async fn toggling_zoom_sets_and_clears_the_active_windows_flag() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();
    assert!(!zoomed(&mut client).await);

    client.zoom(None, None).await.unwrap();
    assert!(zoomed(&mut client).await);
    client.zoom(None, None).await.unwrap();
    assert!(!zoomed(&mut client).await);

    client.zoom(None, Some(true)).await.unwrap();
    client.zoom(None, Some(true)).await.unwrap();
    assert!(zoomed(&mut client).await, "setting on twice keeps it on");
    client.zoom(Some("left"), Some(false)).await.unwrap();
    assert!(!zoomed(&mut client).await);
}

#[tokio::test]
async fn zooming_a_named_pane_focuses_it_first() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();
    assert_eq!(focused(&mut client).await.as_deref(), Some("right"));

    client.zoom(Some("left"), None).await.unwrap();
    assert_eq!(focused(&mut client).await.as_deref(), Some("left"));
    assert!(zoomed(&mut client).await);
    assert!(client.zoom(Some("nosuch"), None).await.is_err());
}

#[tokio::test]
async fn focusing_another_pane_unzooms_unless_the_zoom_is_kept() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();

    client.zoom(None, None).await.unwrap();
    client.focus("left").await.unwrap();
    assert!(!zoomed(&mut client).await, "select-pane unzooms");

    client.zoom(None, None).await.unwrap();
    client.focus_with("right", true).await.unwrap();
    assert!(zoomed(&mut client).await, "select-pane -Z keeps the zoom");
    assert_eq!(focused(&mut client).await.as_deref(), Some("right"));

    client.focus_last_with(true).await.unwrap();
    assert!(zoomed(&mut client).await, "last-pane -Z keeps the zoom");
    client.focus_last().await.unwrap();
    assert!(!zoomed(&mut client).await, "last-pane unzooms");
}

#[tokio::test]
async fn splitting_and_swapping_unzoom_unless_kept() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();

    client.zoom(None, None).await.unwrap();
    client
        .swap_pane_with(Some("left"), Some("right"), true)
        .await
        .unwrap();
    assert!(zoomed(&mut client).await, "swap-pane -Z keeps the zoom");
    client.swap_pane("left", "right").await.unwrap();
    assert!(!zoomed(&mut client).await, "swap-pane unzooms");

    client.zoom(None, None).await.unwrap();
    client.split(split_right("left", "third")).await.unwrap();
    assert!(!zoomed(&mut client).await, "a split unzooms");
}

#[tokio::test]
async fn switching_windows_unzooms_the_window_left_behind() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("main", "cat", &[])).await.unwrap();
    client.zoom(None, None).await.unwrap();
    assert!(zoomed(&mut client).await);

    client
        .new_window(NewWindowOpts {
            name: Some("second".into()),
            spawn: task("other", "cat", &[]),
            focus: true,
        })
        .await
        .unwrap();
    let after = tabs(&mut client).await;
    assert!(!after[0].zoomed, "the window left behind is unzoomed");
    assert!(after[1].is_active && !after[1].zoomed);

    client.select_window(0).await.unwrap();
    assert!(!zoomed(&mut client).await);
}

#[tokio::test]
async fn the_tab_reports_the_focused_pane_and_the_zoom() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("left", "cat", &[])).await.unwrap();
    client.split(split_right("left", "right")).await.unwrap();
    client.zoom(None, None).await.unwrap();

    let tab = tabs(&mut client).await.remove(0);
    assert!(tab.zoomed);
    assert_eq!(tab.focused.as_deref(), Some("right"));
}
