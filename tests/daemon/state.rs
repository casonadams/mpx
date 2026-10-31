//! Session state outside the layout: runtime key bindings, paste buffers and window moves.
use std::time::Duration;

use super::support::{connect, start, task};
use mpx::protocol::{NewWindowOpts, StreamEvent};

#[tokio::test]
async fn bindings_are_stored_by_name_removed_when_forgotten_and_announced_to_terminals() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;

    let (term_tx, mut term_rx) = tokio::sync::mpsc::channel(64);
    let terminal = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = terminal
            .stream_logs_into((None, None), true, term_tx, |e| e)
            .await;
    });
    let (all_tx, mut all_rx) = tokio::sync::mpsc::channel(64);
    let everything = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = everything
            .stream_logs_into((None, None), false, all_tx, |e| e)
            .await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    client
        .set_binding("prefix:y", Some("kill-window"))
        .await
        .unwrap();
    client
        .set_binding("root:F5", Some("next-window"))
        .await
        .unwrap();
    assert_eq!(
        client.get_bindings().await.unwrap(),
        [
            ("prefix:y".to_string(), "kill-window".to_string()),
            ("root:F5".to_string(), "next-window".to_string()),
        ]
    );
    client.set_binding("root:F5", None).await.unwrap();
    assert_eq!(client.get_bindings().await.unwrap().len(), 1);

    let heard = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = term_rx.recv().await {
            if matches!(event, StreamEvent::Bindings) {
                return true;
            }
        }
        false
    })
    .await;
    assert_eq!(heard, Ok(true));

    client.spawn(task("p", "cat", &[])).await.unwrap();
    client.kill("p").await.unwrap();
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(500), all_rx.recv()).await
    {
        assert!(!matches!(event, StreamEvent::Bindings), "{event:?}");
    }
}

#[tokio::test]
async fn buffers_stack_newest_first_and_a_named_buffer_is_replaced() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.set_buffer(None, "one").await.unwrap();
    client.set_buffer(None, "two").await.unwrap();
    client.set_buffer(Some("keep"), "old").await.unwrap();
    client.set_buffer(Some("keep"), "new").await.unwrap();

    let names: Vec<_> = client
        .list_buffers()
        .await
        .unwrap()
        .into_iter()
        .map(|b| (b.name, b.data))
        .collect();
    assert_eq!(
        names,
        [
            ("keep".to_string(), "new".to_string()),
            ("buffer1".to_string(), "two".to_string()),
            ("buffer0".to_string(), "one".to_string()),
        ]
    );
    assert_eq!(client.get_buffer(None).await.unwrap().data, "new");
    assert_eq!(
        client.get_buffer(Some("buffer0")).await.unwrap().data,
        "one"
    );

    client.delete_buffer(None).await.unwrap();
    assert_eq!(client.get_buffer(None).await.unwrap().name, "buffer1");
    assert!(client.get_buffer(Some("missing")).await.is_err());
    assert!(client.delete_buffer(Some("missing")).await.is_err());
}

#[tokio::test]
async fn only_the_newest_fifty_buffers_are_kept() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for i in 0..55 {
        client.set_buffer(None, &i.to_string()).await.unwrap();
    }
    let buffers = client.list_buffers().await.unwrap();
    assert_eq!(buffers.len(), 50);
    assert_eq!(buffers[0].data, "54");
    assert_eq!(buffers[49].data, "5");
}

fn window(label: &str, pane: &str) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.to_string()),
        spawn: task(pane, "cat", &[]),
        focus: true,
    }
}

async fn tabs(client: &mut mpx::client::Client) -> Vec<(String, bool)> {
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    tabs.into_iter().map(|t| (t.name, t.is_active)).collect()
}

#[tokio::test]
async fn moving_a_window_keeps_the_active_window_the_same_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for (label, pane) in [("a", "pa"), ("b", "pb"), ("c", "pc")] {
        client.new_window(window(label, pane)).await.unwrap();
    }
    client.select_window(0).await.unwrap();

    client.move_window(0, 2).await.unwrap();
    assert_eq!(
        tabs(&mut client).await,
        [("b".into(), false), ("c".into(), false), ("a".into(), true)]
    );
    assert!(client.move_window(9, 0).await.is_err());
}
