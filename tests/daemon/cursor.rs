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
async fn cursor_and_scrollback_are_reported_for_terminal_panes_only() {
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
    client.spawn(task("plain", "cat", &[])).await.unwrap();
    let plain = client.status("plain").await.unwrap();
    assert_eq!(plain.cursor, None);
    assert_eq!(plain.history_size, None);
    client.kill("plain").await.unwrap();
}

#[tokio::test]
async fn scrollback_count_stays_exact_as_output_arrives_between_reads() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let script = "stty -echo; echo ready; while read line; do echo \"$line\"; done";
    client
        .spawn(pty("echo", "sh", &["-c", script]))
        .await
        .unwrap();
    logs_until(&mut client, "echo", "ready").await;
    let (_, rows) = client
        .status("echo")
        .await
        .unwrap()
        .size
        .expect("terminal pane reports its size");

    let mut printed = 1usize;
    for (marker, count) in [("one", 30usize), ("two", 25), ("three", 25)] {
        let filler = "x\n".repeat(count - 1);
        client
            .send_input("echo", &format!("{filler}{marker}\n"))
            .await
            .unwrap();
        logs_until(&mut client, "echo", marker).await;
        printed += count;
        let info = client.status("echo").await.unwrap();
        assert_eq!(
            info.history_size,
            Some(printed + 1 - rows as usize),
            "after {marker}"
        );
    }
    client.kill("echo").await.unwrap();
}
