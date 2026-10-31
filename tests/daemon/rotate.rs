//! Window rotation through the daemon client.
use mpx::layout::{Direction, Rect};
use mpx::protocol::SplitOpts;

use super::support::{connect, start, task};

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
