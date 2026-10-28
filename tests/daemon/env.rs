use super::support::{SESSION, connect, exit_code, logs_until, pty, start, task};
use mpx::protocol::SpawnOpts;

#[tokio::test]
async fn task_nodes_receive_mpx_identity_variables() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task(
            "envtask",
            "sh",
            &[
                "-c",
                "echo $MPX_PANE_ID $MPX_PANE $MPX_SESSION $MPX_ENDPOINT",
            ],
        ))
        .await
        .unwrap();
    assert_eq!(exit_code(&mut client, "envtask").await, Some(0));

    let (logs, _) = client.logs("envtask", None, false, None).await.unwrap();
    assert!(
        logs.contains(&format!("%0 envtask {} {}", SESSION, daemon.endpoint)),
        "logs: {logs}"
    );
}

#[tokio::test]
async fn pty_nodes_receive_the_pane_id() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("first", "cat", &[])).await.unwrap();
    client
        .spawn(pty("second", "sh", &["-c", "echo PANE=$MPX_PANE_ID"]))
        .await
        .unwrap();
    let logs = logs_until(&mut client, "second", "PANE=%1").await;
    assert!(logs.contains("PANE=%1"), "logs: {logs}");
    client.kill("first").await.unwrap();
}

#[tokio::test]
async fn spawn_env_is_applied_after_mpx_variables() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(SpawnOpts {
            name: "custom".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "echo $GREETING".into()],
            env: vec![("GREETING".into(), "hi_there".into())],
            ..SpawnOpts::default()
        })
        .await
        .unwrap();
    exit_code(&mut client, "custom").await;
    let (logs, _) = client.logs("custom", None, false, None).await.unwrap();
    assert!(logs.contains("hi_there"), "logs: {logs}");
}
