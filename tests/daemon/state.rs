//! Session state outside the layout: runtime key bindings, paste buffers and window moves.
use std::time::Duration;

use super::support::{connect, logs_until, start, task};
use mpx::protocol::{PasteBufferOpts, StreamEvent};

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
    client.set_buffer(None, "one", false).await.unwrap();
    client.set_buffer(None, "two", false).await.unwrap();
    client.set_buffer(Some("keep"), "old", false).await.unwrap();
    client.set_buffer(Some("keep"), "new", false).await.unwrap();

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
            ("buffer0001".to_string(), "two".to_string()),
            ("buffer0000".to_string(), "one".to_string()),
        ]
    );
    assert_eq!(client.get_buffer(None).await.unwrap().data, "new");
    assert_eq!(
        client.get_buffer(Some("buffer0000")).await.unwrap().data,
        "one"
    );

    client.delete_buffer(None).await.unwrap();
    assert_eq!(client.get_buffer(None).await.unwrap().name, "buffer0001");
    assert!(client.get_buffer(Some("missing")).await.is_err());
    assert!(client.delete_buffer(Some("missing")).await.is_err());
}

#[tokio::test]
async fn only_the_newest_fifty_buffers_are_kept() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for i in 0..55 {
        client
            .set_buffer(None, &i.to_string(), false)
            .await
            .unwrap();
    }
    let buffers = client.list_buffers().await.unwrap();
    assert_eq!(buffers.len(), 50);
    assert_eq!(buffers[0].data, "54");
    assert_eq!(buffers[49].data, "5");
}

#[tokio::test]
async fn set_buffer_appends_renames_and_names_new_buffers_past_taken_names() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.set_buffer(Some("a"), "one", false).await.unwrap();
    client.set_buffer(Some("a"), "two", true).await.unwrap();
    client.set_buffer(None, "three", true).await.unwrap();
    assert_eq!(
        client.get_buffer(Some("a")).await.unwrap().data,
        "onetwothree"
    );

    client.rename_buffer(Some("a"), "b").await.unwrap();
    assert!(client.get_buffer(Some("a")).await.is_err());
    assert_eq!(
        client.get_buffer(Some("b")).await.unwrap().data,
        "onetwothree"
    );
    assert!(client.rename_buffer(Some("missing"), "x").await.is_err());

    client
        .set_buffer(Some("buffer0000"), "taken", false)
        .await
        .unwrap();
    client.set_buffer(None, "auto", false).await.unwrap();
    assert_eq!(client.get_buffer(None).await.unwrap().name, "buffer0001");
    let clash = client.rename_buffer(Some("buffer0001"), "b").await;
    assert!(clash.unwrap_err().contains("already exists"));
}

#[tokio::test]
async fn paste_buffer_pastes_into_a_pane_and_deletes_the_buffer_with_d() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("t", "cat", &[])).await.unwrap();
    client
        .set_buffer(Some("k"), "hi\nthere\n", false)
        .await
        .unwrap();
    let paste = |name: &str| PasteBufferOpts {
        name: Some(name.to_string()),
        pane: "t".to_string(),
        delete: true,
        bracketed: false,
        raw: false,
        separator: None,
    };
    assert!(client.paste_buffer(paste("gone")).await.is_err());
    client.paste_buffer(paste("k")).await.unwrap();
    logs_until(&mut client, "t", "hi\nthere").await;
    assert!(client.get_buffer(Some("k")).await.is_err());
}
