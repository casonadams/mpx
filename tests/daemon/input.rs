use super::support::{connect, logs_until, pty, start, task};
use mpx::protocol::{SpawnOpts, WaitCondition};

#[tokio::test]
async fn literal_send_keys_are_written_as_text() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("lit", "cat", &[])).await.unwrap();
    client
        .send_keys("lit", vec!["lit".into(), "eral".into(), "\n".into()], true)
        .await
        .unwrap();
    let logs = logs_until(&mut client, "lit", "literal").await;
    assert!(logs.contains("literal"), "logs: {logs}");
    client.kill("lit").await.unwrap();
}

#[tokio::test]
async fn named_keys_are_encoded_for_the_pty() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("keys", "cat", &[])).await.unwrap();
    client
        .send_keys("keys", vec!["o".into(), "k".into(), "Enter".into()], false)
        .await
        .unwrap();
    let logs = logs_until(&mut client, "keys", "ok").await;
    assert!(logs.contains("ok"), "logs: {logs}");
    client.kill("keys").await.unwrap();
}

#[tokio::test]
async fn paste_is_bracketed_when_the_program_enables_it() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty(
            "bracketed",
            "sh",
            &["-c", "printf '\\033[?2004hREADY\\n'; cat"],
        ))
        .await
        .unwrap();
    client
        .wait(
            "bracketed",
            WaitCondition::Match {
                pattern: "READY".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();

    client
        .paste("bracketed", "pasted_text", false)
        .await
        .unwrap();
    let logs = logs_until(&mut client, "bracketed", "pasted_text").await;
    assert!(logs.contains("200~"), "paste was not bracketed: {logs:?}");
    client.kill("bracketed").await.unwrap();
}

#[tokio::test]
async fn paste_is_plain_without_bracketed_mode_and_submit_presses_enter() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty("plain", "sh", &["-c", "echo READY; cat"]))
        .await
        .unwrap();
    client
        .wait(
            "plain",
            WaitCondition::Match {
                pattern: "READY".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();

    client.paste("plain", "submitted_line", true).await.unwrap();
    let logs = logs_until(&mut client, "plain", "submitted_line").await;
    assert!(logs.contains("submitted_line"), "logs: {logs}");
    assert!(!logs.contains("200~"));
    client.kill("plain").await.unwrap();
}

#[tokio::test]
async fn task_paste_submit_appends_newline() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(SpawnOpts {
            name: "tasked".into(),
            command: "cat".into(),
            ..SpawnOpts::default()
        })
        .await
        .unwrap();
    client.paste("tasked", "via_task", true).await.unwrap();
    let logs = logs_until(&mut client, "tasked", "via_task\n").await;
    assert!(logs.contains("via_task\n"), "logs: {logs}");
    client.kill("tasked").await.unwrap();
}
