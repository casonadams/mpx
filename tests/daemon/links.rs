//! Shared windows (`link-window`, `unlink-window`), manual window sizes (`resize-window`) and the
//! snapshot of a window linked at several indices, through the daemon client.
use tokio::sync::mpsc;

use mpx::client::Client;
use mpx::config::{Config, WindowOptions};
use mpx::layout::LinkPlace;
use mpx::protocol::{ClientInfo, Side, StreamEvent, WaitCondition, WindowFit, WindowResize};

use super::support::{
    assert_refused, connect, fresh, pty, split_right, start_on, start_with, tabs, window,
};

/// The index and name of every window tab, in index order.
async fn numbered(client: &mut Client) -> Vec<(usize, String)> {
    tabs(client)
        .await
        .into_iter()
        .map(|t| (t.index, t.name))
        .collect()
}

/// The panes the active window lays out, in layout order.
async fn panes_shown(client: &mut Client) -> Vec<String> {
    client
        .layout(80, 24)
        .await
        .unwrap()
        .0
        .into_iter()
        .map(|p| p.0)
        .collect()
}

/// A pane of `width` by `height` cells: the size of the active window's only pane.
async fn pane_size(client: &mut Client) -> (u16, u16) {
    let (rects, _, _) = client.layout(80, 24).await.unwrap();
    let rect = rects.first().expect("a pane").1;
    (rect.width, rect.height)
}

/// Whether pane `name` exits within two seconds: a killed pane is signalled and stays in history.
async fn pane_exits(client: &mut Client, name: &str) -> bool {
    let outcome = client
        .wait(name, WaitCondition::Exit, Some(2_000))
        .await
        .expect("wait");
    outcome.satisfied
}

/// Subscribes an attached terminal of `width` by `height` cells; keep the receiver alive.
async fn attach_sized(
    endpoint: &str,
    name: &str,
    width: u16,
    height: u16,
) -> mpsc::Receiver<StreamEvent> {
    let info = ClientInfo {
        name: name.into(),
        width,
        height,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let client = connect(endpoint).await;
    tokio::spawn(async move {
        let _ = client.stream_as_client(info, false, false, tx, |e| e).await;
    });
    rx
}

/// Waits (bounded) until `count` terminals are attached.
async fn await_clients(client: &mut Client, count: usize) {
    for _ in 0..200 {
        if client.list_clients().await.unwrap().len() == count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("{count} clients never attached");
}

fn resize(
    width: Option<u16>,
    height: Option<u16>,
    side: Option<Side>,
    cells: Option<u16>,
    fit: Option<WindowFit>,
) -> WindowResize {
    WindowResize {
        width,
        height,
        side,
        cells,
        fit,
    }
}

#[tokio::test]
async fn a_linked_window_shows_the_same_panes_at_both_indices() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.split(split_right("pa", "pb")).await.unwrap();

    client
        .link_window(None, None, LinkPlace::default())
        .await
        .unwrap();

    assert_eq!(
        numbered(&mut client).await,
        [(0, "a".to_string()), (1, "a".to_string())]
    );
    client.select_window(1).await.unwrap();
    assert_eq!(panes_shown(&mut client).await, ["pa", "pb"]);
}

#[tokio::test]
async fn killing_a_pane_removes_it_from_every_index_of_its_window() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.split(split_right("pa", "pb")).await.unwrap();
    client
        .link_window(None, None, LinkPlace::default())
        .await
        .unwrap();

    client.kill("pb").await.unwrap();
    for number in [0, 1] {
        client.select_window(number).await.unwrap();
        assert_eq!(panes_shown(&mut client).await, ["pa"], "index {number}");
    }

    client.kill("pa").await.unwrap();
    assert!(numbered(&mut client).await.is_empty());
}

#[tokio::test]
async fn unlink_keeps_the_window_at_its_other_index_and_the_last_index_needs_kill() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client
        .link_window(None, None, LinkPlace::default())
        .await
        .unwrap();

    client.unlink_window(Some(1), false).await.unwrap();
    assert_eq!(numbered(&mut client).await, [(0, "a".to_string())]);
    assert!(client.list().await.unwrap().iter().any(|n| n.name == "pa"));

    let refused = client
        .unlink_window(Some(0), false)
        .await
        .expect_err("the only index");
    assert!(refused.contains("use -k"), "{refused}");
    assert_eq!(numbered(&mut client).await.len(), 1);

    client.unlink_window(Some(0), true).await.unwrap();
    assert!(numbered(&mut client).await.is_empty());
    assert!(pane_exits(&mut client, "pa").await, "the killed pane exits");
}

#[tokio::test]
async fn unlinking_an_index_renumbers_the_indices_after_it() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();
    client.new_window(window("c", "pc")).await.unwrap();
    client
        .link_window(Some(0), Some(5), LinkPlace::default())
        .await
        .unwrap();
    assert_eq!(
        numbered(&mut client)
            .await
            .iter()
            .map(|t| t.0)
            .collect::<Vec<_>>(),
        [0, 1, 2, 5]
    );

    client.unlink_window(Some(1), true).await.unwrap();
    assert_eq!(
        numbered(&mut client).await,
        [
            (0, "a".to_string()),
            (1, "c".to_string()),
            (2, "a".to_string()),
        ]
    );
}

#[tokio::test]
async fn with_renumber_windows_off_the_gap_stays() {
    let config = Config {
        window: WindowOptions {
            renumber_windows: false,
            ..WindowOptions::default()
        },
        ..Config::default()
    };
    let daemon = start_with(config).await;
    let mut client = connect(&daemon.endpoint).await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();
    client.new_window(window("c", "pc")).await.unwrap();
    client
        .link_window(Some(0), Some(5), LinkPlace::default())
        .await
        .unwrap();

    client.unlink_window(Some(1), true).await.unwrap();
    assert_eq!(
        numbered(&mut client)
            .await
            .iter()
            .map(|t| t.0)
            .collect::<Vec<_>>(),
        [0, 2, 5]
    );
}

#[tokio::test]
async fn a_link_made_last_is_the_window_last_window_returns_to() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();
    client.select_window(0).await.unwrap();

    client
        .link_window(Some(1), None, LinkPlace::default())
        .await
        .unwrap();
    client.last_window().await.unwrap();

    let active: Vec<usize> = tabs(&mut client)
        .await
        .into_iter()
        .filter(|t| t.is_active)
        .map(|t| t.index)
        .collect();
    assert_eq!(
        active,
        [0],
        "the link took focus; last-window returns to index 0"
    );
}

#[tokio::test]
async fn linking_a_missing_window_is_refused() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    let result = client
        .link_window(Some(7), Some(1), LinkPlace::default())
        .await;
    assert_refused(result, "No window 7");
}

#[tokio::test]
async fn a_linked_window_is_restored_once_and_shown_at_each_index() {
    let state = tempfile::tempdir().unwrap();
    let first = start_on(state.path(), Config::default()).await;
    let mut client = connect(&first.endpoint).await;
    client.spawn(pty("keeper", "cat", &[])).await.unwrap();
    client
        .link_window(None, None, LinkPlace::default())
        .await
        .unwrap();
    client.shutdown().await.unwrap();

    let second = start_on(state.path(), Config::default()).await;
    let mut client = connect(&second.endpoint).await;
    let shown = numbered(&mut client).await;
    let indices: Vec<usize> = shown.iter().map(|(index, _)| *index).collect();
    assert_eq!(indices, [0, 1]);
    assert_eq!(
        shown[0].1, shown[1].1,
        "both indices show the one restored window"
    );
    let names: Vec<String> = client
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert_eq!(names, ["keeper"], "the window's pane is spawned once");
    client.select_window(1).await.unwrap();
    assert_eq!(panes_shown(&mut client).await, ["keeper"]);
}

#[tokio::test]
async fn resize_window_pins_the_size_of_that_window_only() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();
    client.select_window(0).await.unwrap();

    client
        .resize_window(None, resize(Some(30), Some(10), None, None, None))
        .await
        .unwrap();
    assert_eq!(pane_size(&mut client).await, (30, 10));

    client.select_window(1).await.unwrap();
    assert_eq!(pane_size(&mut client).await, (80, 24));
}

#[tokio::test]
async fn resize_window_adjusts_from_the_current_size() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.layout(80, 24).await.unwrap();

    client
        .resize_window(None, resize(None, None, Some(Side::Right), Some(5), None))
        .await
        .unwrap();
    assert_eq!(pane_size(&mut client).await, (85, 24));

    client
        .resize_window(None, resize(None, None, Some(Side::Up), Some(4), None))
        .await
        .unwrap();
    assert_eq!(pane_size(&mut client).await, (85, 20));
}

#[tokio::test]
async fn resize_window_fit_takes_the_largest_or_smallest_attached_client() {
    let (daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    let _small = attach_sized(&daemon.endpoint, "small", 50, 20).await;
    let _large = attach_sized(&daemon.endpoint, "large", 100, 40).await;
    await_clients(&mut client, 2).await;

    client
        .resize_window(
            None,
            resize(None, None, None, None, Some(WindowFit::Smallest)),
        )
        .await
        .unwrap();
    assert_eq!(pane_size(&mut client).await, (50, 20));

    client
        .resize_window(
            None,
            resize(None, None, None, None, Some(WindowFit::Largest)),
        )
        .await
        .unwrap();
    assert_eq!(pane_size(&mut client).await, (100, 40));
}

#[tokio::test]
async fn resize_window_refuses_a_size_out_of_range() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    let result = client
        .resize_window(None, resize(Some(0), None, None, None, None))
        .await;
    assert_refused(result, "out of range");
}

#[tokio::test]
async fn killing_a_pane_that_empties_no_window_keeps_the_index_numbers() {
    let (_daemon, mut client) = fresh().await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.split(split_right("pa", "pb")).await.unwrap();
    client
        .link_window(Some(0), Some(5), LinkPlace::default())
        .await
        .unwrap();

    client.kill("pb").await.unwrap();
    assert_eq!(
        numbered(&mut client)
            .await
            .iter()
            .map(|t| t.0)
            .collect::<Vec<_>>(),
        [0, 5]
    );
}
