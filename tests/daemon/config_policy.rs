//! Config the daemon acts on: exit policy, scrollback, window naming, and the default program and
//! terminal of new panes.
use mpx::client::Client;
use mpx::config::{Config, RemainOnExit};
use mpx::protocol::{NewWindowOpts, SpawnOpts};

use super::support::{connect, exit_code, logs_until, pty, start_with, task};

async fn pane_names(client: &mut Client) -> Vec<String> {
    client
        .layout(80, 24)
        .await
        .unwrap()
        .0
        .into_iter()
        .map(|(name, _, _)| name)
        .collect()
}

#[tokio::test]
async fn remain_on_exit_failed_keeps_only_panes_that_exit_non_zero() {
    let daemon = start_with(Config {
        remain_on_exit: RemainOnExit::Failed,
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("ok", "true", &[])).await.unwrap();
    client
        .spawn(task("bad", "sh", &["-c", "exit 3"]))
        .await
        .unwrap();
    assert_eq!(exit_code(&mut client, "ok").await, Some(0));
    assert_eq!(exit_code(&mut client, "bad").await, Some(3));
    assert_eq!(pane_names(&mut client).await, ["bad"]);
}

#[tokio::test]
async fn scrollback_lines_limits_the_history_a_pane_keeps() {
    let daemon = start_with(Config {
        scrollback_lines: 3,
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    let script = "for i in $(seq -w 0 29); do echo L$i; done; sleep 30";
    client
        .spawn(pty("limited", "sh", &["-c", script]))
        .await
        .unwrap();
    logs_until(&mut client, "limited", "L29").await;

    let (text, _) = client.logs("limited", None, true, None).await.unwrap();
    assert!(
        !text.contains("L03") && text.contains("L04") && text.contains("L29"),
        "three history lines (L04-L06), then the screen: {text}"
    );
}

#[tokio::test]
async fn automatic_rename_off_keeps_the_name_a_window_was_created_with() {
    let daemon = start_with(Config {
        automatic_rename: false,
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(NewWindowOpts {
            name: None,
            spawn: pty("auto-pane", "sh", &["-c", "exec sleep 30"]),
            focus: true,
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(client.layout(80, 24).await.unwrap().2[0].name, "sh");
}

#[tokio::test]
async fn default_shell_command_and_terminal_apply_to_panes_started_without_a_command() {
    let daemon = start_with(Config {
        default_terminal: "screen-256color".to_string(),
        default_shell: Some("/bin/sh".to_string()),
        default_command: Some("printf 'READY %s' \"$TERM\"; sleep 30".to_string()),
        ..Config::default()
    })
    .await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(SpawnOpts {
            name: "cmd".into(),
            pty: true,
            ..SpawnOpts::default()
        })
        .await
        .unwrap();
    let logs = logs_until(&mut client, "cmd", "READY screen-256color").await;
    assert!(logs.contains("READY screen-256color"), "{logs}");
}
