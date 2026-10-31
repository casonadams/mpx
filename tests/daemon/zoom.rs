//! Zoom is the daemon's per-window flag: toggled by requests, followed by every terminal through
//! the layout it reports, and cleared by the changes that move focus or rearrange the layout.
use super::support::{TestDaemon, split_right, started_with, task};
use mpx::client::Client;
use mpx::layout::WindowTab;
use mpx::protocol::NewWindowOpts;

/// A daemon with a `left` pane and a focused `right` pane split off it.
async fn two_panes() -> (TestDaemon, Client) {
    let (daemon, mut client) = started_with(task("left", "cat", &[])).await;
    client.split(split_right("left", "right")).await.unwrap();
    (daemon, client)
}

/// The focused pane's name and the window tabs, from one layout request.
async fn screen(client: &mut Client) -> (Option<String>, Vec<WindowTab>) {
    let (rects, _, tabs) = client.layout(80, 24).await.unwrap();
    (rects.into_iter().find(|p| p.2).map(|p| p.0), tabs)
}

async fn tabs(client: &mut Client) -> Vec<WindowTab> {
    screen(client).await.1
}

async fn focused(client: &mut Client) -> Option<String> {
    screen(client).await.0
}

/// Whether the active window's flag is set.
async fn zoomed(client: &mut Client) -> bool {
    tabs(client).await.iter().any(|t| t.is_active && t.zoomed)
}

async fn zoom(client: &mut Client, pane: Option<&str>, on: Option<bool>) {
    client.zoom(pane, on).await.unwrap();
}

async fn assert_zoomed(client: &mut Client, expected: bool, why: &str) {
    assert_eq!(zoomed(client).await, expected, "{why}");
}

#[tokio::test]
async fn toggling_zoom_sets_and_clears_the_active_windows_flag() {
    let (_daemon, mut client) = two_panes().await;
    assert_zoomed(&mut client, false, "starts unzoomed").await;

    zoom(&mut client, None, None).await;
    assert_zoomed(&mut client, true, "toggles on").await;
    zoom(&mut client, None, None).await;
    assert_zoomed(&mut client, false, "toggles off").await;

    zoom(&mut client, None, Some(true)).await;
    zoom(&mut client, None, Some(true)).await;
    assert_zoomed(&mut client, true, "setting on twice keeps it on").await;
    zoom(&mut client, Some("left"), Some(false)).await;
    assert_zoomed(&mut client, false, "setting off clears it").await;
}

#[tokio::test]
async fn zooming_a_named_pane_focuses_it_first() {
    let (_daemon, mut client) = two_panes().await;
    assert_eq!(focused(&mut client).await.as_deref(), Some("right"));

    zoom(&mut client, Some("left"), None).await;
    assert_eq!(focused(&mut client).await.as_deref(), Some("left"));
    assert_zoomed(&mut client, true, "zooming sets the flag").await;
    assert!(client.zoom(Some("nosuch"), None).await.is_err());
}

#[tokio::test]
async fn focusing_another_pane_unzooms_unless_the_zoom_is_kept() {
    let (_daemon, mut client) = two_panes().await;

    zoom(&mut client, None, None).await;
    client.focus("left").await.unwrap();
    assert_zoomed(&mut client, false, "select-pane unzooms").await;

    zoom(&mut client, None, None).await;
    client.focus_with("right", true).await.unwrap();
    assert_zoomed(&mut client, true, "select-pane -Z keeps the zoom").await;
    assert_eq!(focused(&mut client).await.as_deref(), Some("right"));

    client.focus_last_with(true).await.unwrap();
    assert_zoomed(&mut client, true, "last-pane -Z keeps the zoom").await;
    client.focus_last().await.unwrap();
    assert_zoomed(&mut client, false, "last-pane unzooms").await;
}

#[tokio::test]
async fn splitting_and_swapping_unzoom_unless_kept() {
    let (_daemon, mut client) = two_panes().await;

    zoom(&mut client, None, None).await;
    client
        .swap_pane_with(Some("left"), Some("right"), true)
        .await
        .unwrap();
    assert_zoomed(&mut client, true, "swap-pane -Z keeps the zoom").await;
    client.swap_pane("left", "right").await.unwrap();
    assert_zoomed(&mut client, false, "swap-pane unzooms").await;

    zoom(&mut client, None, None).await;
    client.split(split_right("left", "third")).await.unwrap();
    assert_zoomed(&mut client, false, "a split unzooms").await;
}

#[tokio::test]
async fn switching_windows_unzooms_the_window_left_behind() {
    let (_daemon, mut client) = started_with(task("main", "cat", &[])).await;
    zoom(&mut client, None, None).await;
    assert_zoomed(&mut client, true, "zoomed before switching").await;

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
    assert_zoomed(&mut client, false, "selecting back keeps it unzoomed").await;
}

#[tokio::test]
async fn the_tab_reports_the_focused_pane_and_the_zoom() {
    let (_daemon, mut client) = two_panes().await;
    zoom(&mut client, None, None).await;

    let tab = tabs(&mut client).await.remove(0);
    assert!(tab.zoomed);
    assert_eq!(tab.focused.as_deref(), Some("right"));
}
