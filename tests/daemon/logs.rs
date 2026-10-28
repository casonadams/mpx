use super::support::{connect, logs_until, pty, start, task};

#[tokio::test]
async fn since_returns_only_new_output_with_absolute_offsets() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("cat-logs", "cat", &[])).await.unwrap();

    client.send_input("cat-logs", "one\n").await.unwrap();
    logs_until(&mut client, "cat-logs", "one").await;
    let (_, offset) = client.logs("cat-logs", None, false, None).await.unwrap();
    assert!(offset > 0);

    client.send_input("cat-logs", "two\n").await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let (delta, next) = loop {
        let (delta, next) = client
            .logs("cat-logs", None, false, Some(offset))
            .await
            .unwrap();
        if delta.contains("two") || std::time::Instant::now() >= deadline {
            break (delta, next);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert!(delta.contains("two"), "delta: {delta}");
    assert!(!delta.contains("one"), "delta repeats old output: {delta}");
    assert!(next > offset);

    let (again, same) = client
        .logs("cat-logs", None, false, Some(next))
        .await
        .unwrap();
    assert!(again.is_empty());
    assert_eq!(same, next);
    client.kill("cat-logs").await.unwrap();
}

#[tokio::test]
async fn plain_pty_logs_are_free_of_escape_sequences() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty(
            "colored",
            "sh",
            &["-c", "printf '\\033[31mred_text\\033[0m\\n'"],
        ))
        .await
        .unwrap();
    logs_until(&mut client, "colored", "red_text").await;

    let (plain, _) = client.logs("colored", None, true, None).await.unwrap();
    assert!(plain.contains("red_text"), "plain: {plain}");
    assert!(
        !plain.contains('\u{1b}'),
        "plain still has escapes: {plain:?}"
    );

    let (raw, _) = client.logs("colored", None, false, None).await.unwrap();
    assert!(raw.contains('\u{1b}'), "raw logs keep escapes");
}

#[tokio::test]
async fn plain_since_strips_escapes_from_the_delta() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("delta", "cat", &[])).await.unwrap();
    let (_, start) = client.logs("delta", None, true, None).await.unwrap();

    client
        .send_input("delta", "\u{1b}[1mbold_word\u{1b}[0m\n")
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let (delta, _) = client.logs("delta", None, true, Some(start)).await.unwrap();
        if delta.contains("bold_word") || std::time::Instant::now() >= deadline {
            assert!(delta.contains("bold_word"), "delta: {delta:?}");
            assert!(!delta.contains('\u{1b}'), "delta: {delta:?}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    client.kill("delta").await.unwrap();
}

#[tokio::test]
async fn tail_limits_task_output_to_last_lines() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("lines", "sh", &["-c", "echo l1; echo l2; echo l3"]))
        .await
        .unwrap();
    super::support::exit_code(&mut client, "lines").await;
    let (tail, _) = client.logs("lines", Some(2), false, None).await.unwrap();
    assert_eq!(tail, "l2\nl3\n");
}
