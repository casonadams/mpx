use super::support::{connect, start, task};
use mpx::layout::Direction;
use mpx::protocol::SplitOpts;

#[tokio::test]
async fn pane_ids_count_up_from_zero_and_resolve_as_targets() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let first = client.spawn(task("alpha", "cat", &[])).await.unwrap();
    let second = client
        .split(SplitOpts {
            target: Some("alpha".into()),
            spawn: task("beta", "cat", &[]),
            direction: Direction::Horizontal,
            percent: None,
            focus: true,
        })
        .await
        .unwrap();

    assert_eq!(first.id, 0);
    assert_eq!(second.id, 1);
    assert_eq!(second.pane_ref(), "%1");

    let by_ref = client.status("%1").await.unwrap();
    assert_eq!(by_ref.name, "beta");
    assert_eq!(client.status("%0").await.unwrap().name, "alpha");
    assert!(client.status("%9").await.is_err());
    assert!(client.status("%x").await.is_err());

    client.send_input("%0", "").await.unwrap();
    client.kill("%1").await.unwrap();
    assert!(
        client.status("beta").await.is_ok(),
        "killed pane stays in history"
    );
    client.kill("alpha").await.unwrap();
}

#[tokio::test]
async fn pane_names_starting_with_percent_are_rejected() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    assert!(client.spawn(task("%bad", "cat", &[])).await.is_err());
}

#[tokio::test]
async fn ids_keep_increasing_after_nodes_exit() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("gone", "true", &[])).await.unwrap();
    client
        .wait("gone", mpx::protocol::WaitCondition::Exit, Some(2_000))
        .await
        .unwrap();
    let next = client.spawn(task("next", "cat", &[])).await.unwrap();
    assert_eq!(next.id, 1);
    client.kill("next").await.unwrap();
}
