use super::support::{connect, logs_until, pty, start, task};

#[tokio::test]
async fn terminal_pane_reports_cursor_and_scrollback() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty(
            "lines",
            "sh",
            &[
                "-c",
                "for i in $(seq 1 50); do echo line$i; done; printf abc; sleep 30",
            ],
        ))
        .await
        .unwrap();
    logs_until(&mut client, "lines", "abc").await;

    let info = client.status("lines").await.unwrap();
    let (_, rows) = info.size.expect("terminal pane reports its size");
    assert_eq!(info.cursor, Some((3, rows - 1)));
    assert_eq!(info.history_size, Some(51 - rows as usize));
    client.kill("lines").await.unwrap();
}

#[tokio::test]
async fn cursor_and_scrollback_follow_new_output() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("echo", "cat", &[])).await.unwrap();

    let before = client.status("echo").await.unwrap();
    assert_eq!(before.cursor, Some((0, 0)));
    assert_eq!(before.history_size, Some(0));

    client.send_input("echo", "abc").await.unwrap();
    logs_until(&mut client, "echo", "abc").await;
    let typed = client.status("echo").await.unwrap();
    assert_eq!(typed.cursor, Some((3, 0)));
    assert_eq!(typed.history_size, Some(0));

    client
        .send_input("echo", &format!("{}end\n", "\n".repeat(40)))
        .await
        .unwrap();
    logs_until(&mut client, "echo", "end").await;
    let scrolled = client.status("echo").await.unwrap();
    assert!(
        scrolled.history_size.unwrap_or(0) > 0,
        "history_size: {:?}",
        scrolled.history_size
    );
    client.kill("echo").await.unwrap();
}

#[tokio::test]
async fn task_pane_reports_no_cursor_or_scrollback() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("plain", "cat", &[])).await.unwrap();

    let info = client.status("plain").await.unwrap();
    assert_eq!(info.cursor, None);
    assert_eq!(info.history_size, None);
    client.kill("plain").await.unwrap();
}
