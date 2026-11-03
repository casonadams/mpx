//! Window numbers with `renumber-windows` on (the default) and off, and `move-window -r`.
use mpx::client::Client;
use mpx::config::Config;
use mpx::protocol::NewWindowOpts;

use super::support::{connect, start, start_with, task};

fn window(label: &str) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.to_string()),
        spawn: task(&format!("p{label}"), "cat", &[]),
        focus: true,
    }
}

fn numbers_off() -> Config {
    Config {
        renumber_windows: false,
        ..Config::default()
    }
}

async fn open(client: &mut Client, labels: &[&str]) {
    for label in labels {
        client.new_window(window(label)).await.unwrap();
    }
}

/// Window numbers and names, in window order.
async fn numbered(client: &mut Client) -> Vec<(usize, String)> {
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    tabs.into_iter().map(|t| (t.index, t.name)).collect()
}

#[tokio::test]
async fn renumber_on_close_packs_the_numbers_again() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    open(&mut client, &["a", "b", "c"]).await;

    client.kill_window(Some(1)).await.unwrap();

    assert_eq!(
        numbered(&mut client).await,
        [(0, "a".to_string()), (1, "c".to_string())]
    );
}

#[tokio::test]
async fn renumber_off_keeps_the_numbers_of_the_windows_after_a_close() {
    let daemon = start_with(numbers_off()).await;
    let mut client = connect(&daemon.endpoint).await;
    open(&mut client, &["a", "b", "c"]).await;

    client.kill_window(Some(1)).await.unwrap();

    assert_eq!(
        numbered(&mut client).await,
        [(0, "a".to_string()), (2, "c".to_string())]
    );
    client.select_window(2).await.unwrap();
    let err = client.select_window(1).await.unwrap_err();
    assert!(err.contains("No window 1"), "{err}");
}

#[tokio::test]
async fn renumber_off_new_window_fills_the_lowest_gap() {
    let daemon = start_with(numbers_off()).await;
    let mut client = connect(&daemon.endpoint).await;
    open(&mut client, &["a", "b", "c"]).await;
    client.kill_window(Some(1)).await.unwrap();

    open(&mut client, &["d"]).await;

    assert_eq!(
        numbered(&mut client).await,
        [(0, "a".to_string()), (1, "d".to_string()), (2, "c".to_string())]
    );
}

#[tokio::test]
async fn move_window_r_compacts_the_numbers() {
    let daemon = start_with(numbers_off()).await;
    let mut client = connect(&daemon.endpoint).await;
    open(&mut client, &["a", "b", "c"]).await;
    client.kill_window(Some(1)).await.unwrap();

    client.renumber_windows().await.unwrap();

    assert_eq!(
        numbered(&mut client).await,
        [(0, "a".to_string()), (1, "c".to_string())]
    );
}
