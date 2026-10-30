use std::time::Duration;

use super::support::{connect, exit_code, pty, start, start_on, start_with, task};
use mpx::config::Config;
use mpx::protocol::{NodeState, SpawnOpts};

#[tokio::test]
async fn shutdown_kills_nodes_and_run_daemon_returns() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = mpx::ipc::endpoint_in(dir.path(), "shut");
    let state_dir = dir.path().join("state");
    let server = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            mpx::daemon::run_daemon(
                &endpoint,
                "shut",
                &state_dir,
                mpx::config::Config::default(),
            )
            .await
        }
    });

    let mut client = loop {
        if let Ok(c) = mpx::client::Client::connect(&endpoint).await {
            break c;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    client
        .spawn(task("sleeper", "sleep", &["30"]))
        .await
        .unwrap();
    client.shutdown().await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("run_daemon returns after shutdown")
        .unwrap();
    assert!(result.is_ok());
    assert!(!std::path::Path::new(&endpoint).exists(), "socket removed");
}

#[tokio::test]
async fn remain_on_exit_keeps_pane_and_logs() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(SpawnOpts {
            name: "kept".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "echo kept_output; exit 4".into()],
            remain_on_exit: Some(true),
            ..SpawnOpts::default()
        })
        .await
        .unwrap();
    assert_eq!(exit_code(&mut client, "kept").await, Some(4));

    let info = client.status("kept").await.unwrap();
    assert_eq!(info.state, NodeState::Exited);
    assert_eq!(info.exit_code, Some(4));

    let (layout, _, _) = client.layout(80, 24).await.unwrap();
    assert_eq!(layout.len(), 1, "exited pane stays on screen");

    let (logs, _) = client.logs("kept", None, false, None).await.unwrap();
    assert!(logs.contains("kept_output"), "logs: {logs}");

    client.kill("kept").await.unwrap();
    assert!(
        client.status("kept").await.is_err(),
        "kill removes an exited pane"
    );
}

#[tokio::test]
async fn config_default_keeps_panes_after_exit() {
    let daemon = start_with(Config {
        remain_on_exit: true.into(),
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("config-kept", "true", &[]))
        .await
        .unwrap();
    exit_code(&mut client, "config-kept").await;
    assert_eq!(
        client.status("config-kept").await.unwrap().state,
        NodeState::Exited
    );
    client.kill("config-kept").await.unwrap();
}

async fn names_after_restart(state: &std::path::Path, config: Config) -> Vec<String> {
    let daemon = start_on(state, config).await;
    let mut client = connect(&daemon.endpoint).await;
    let mut names: Vec<_> = client
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_cleanly_stopped_session_comes_back_on_the_next_start_unless_restore_is_off() {
    let state = tempfile::tempdir().unwrap();
    let first = start_on(state.path(), Config::default()).await;
    let mut client = connect(&first.endpoint).await;
    client.spawn(pty("keeper", "cat", &[])).await.unwrap();
    client.shutdown().await.unwrap();

    let off = Config {
        restore: false,
        ..Config::default()
    };
    assert!(names_after_restart(state.path(), off).await.is_empty());
    assert_eq!(
        names_after_restart(state.path(), Config::default()).await,
        ["keeper"]
    );
}

#[tokio::test]
async fn reload_config_changes_defaults_for_later_panes_and_rejects_bad_files() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");

    std::fs::write(&file, "remain_on_exit = true\n").unwrap();
    client.reload_config(Some(&file)).await.unwrap();
    client.spawn(task("kept", "true", &[])).await.unwrap();
    exit_code(&mut client, "kept").await;
    assert_eq!(client.status("kept").await.unwrap().state, NodeState::Exited);

    std::fs::write(&file, "remain_on_exit = [").unwrap();
    assert!(client.reload_config(Some(&file)).await.is_err());
    client.spawn(task("still-kept", "true", &[])).await.unwrap();
    exit_code(&mut client, "still-kept").await;
    assert_eq!(
        client.status("still-kept").await.unwrap().state,
        NodeState::Exited
    );
}

#[tokio::test]
async fn exited_history_is_capped_and_evicts_oldest() {
    let daemon = start_with(Config {
        history_limit: 2,
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    for name in ["h1", "h2", "h3"] {
        client.spawn(task(name, "true", &[])).await.unwrap();
        exit_code(&mut client, name).await;
    }

    let names: Vec<String> = client
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert_eq!(names.len(), 2, "names: {names:?}");
    assert!(names.contains(&"h2".to_string()) && names.contains(&"h3".to_string()));
    assert!(client.status("h1").await.is_err());
}

#[tokio::test]
async fn killing_a_running_task_records_it_as_exited() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("victim", "sleep", &["30"]))
        .await
        .unwrap();
    client.kill("victim").await.unwrap();
    let info = client.status("victim").await.unwrap();
    assert_eq!(info.state, NodeState::Exited);
}
