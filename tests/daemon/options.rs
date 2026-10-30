//! tmux options the daemon acts on: exit policy, scrollback, activity monitoring, window naming,
//! synchronized input, and the default program and terminal of new panes.
use std::time::Duration;

use mpx::client::Client;
use mpx::layout::WindowTab;
use mpx::protocol::{NewWindowOpts, SpawnOpts, VarKind};

use super::support::{connect, exit_code, logs_until, pty, start, task};

async fn set(client: &mut Client, name: &str, value: &str) -> Result<(), String> {
    client.set_var(VarKind::Option, name, Some(value)).await
}

fn window(name: Option<&str>, spawn: SpawnOpts, focus: bool) -> NewWindowOpts {
    NewWindowOpts {
        name: name.map(str::to_string),
        spawn,
        focus,
    }
}

async fn tabs(client: &mut Client) -> Vec<WindowTab> {
    client.layout(80, 24).await.unwrap().2
}

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

/// Window tabs once `check` holds for them; panics when it does not hold in time.
async fn tabs_until(client: &mut Client, check: impl Fn(&[WindowTab]) -> bool) -> Vec<WindowTab> {
    for _ in 0..150 {
        let current = tabs(client).await;
        if check(&current) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("window tabs did not reach the expected state");
}

#[tokio::test]
async fn remain_on_exit_failed_keeps_only_panes_that_exit_non_zero() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    set(&mut client, "remain-on-exit", "failed").await.unwrap();
    client.spawn(task("ok", "true", &[])).await.unwrap();
    client.spawn(task("bad", "sh", &["-c", "exit 3"])).await.unwrap();
    assert_eq!(exit_code(&mut client, "ok").await, Some(0));
    assert_eq!(exit_code(&mut client, "bad").await, Some(3));
    assert_eq!(pane_names(&mut client).await, ["bad"]);
}

#[tokio::test]
async fn remain_on_exit_is_decided_when_a_pane_exits_not_when_it_started() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("late", "sh", &["-c", "sleep 0.3"]))
        .await
        .unwrap();
    set(&mut client, "remain-on-exit", "on").await.unwrap();
    assert_eq!(exit_code(&mut client, "late").await, Some(0));
    assert_eq!(pane_names(&mut client).await, ["late"]);
}

#[tokio::test]
async fn invalid_option_values_are_refused_and_not_stored() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let err = set(&mut client, "remain-on-exit", "maybe").await.unwrap_err();
    assert_eq!(err, "invalid value: maybe");
    let err = set(&mut client, "monitor-silence", "5").await.unwrap_err();
    assert_eq!(err, "monitor-silence is not supported by mpx");
    let err = set(&mut client, "default-shell", "zsh").await.unwrap_err();
    assert_eq!(err, "invalid value: zsh");
    assert!(client.get_vars(VarKind::Option).await.unwrap().is_empty());
}

#[tokio::test]
async fn history_limit_applies_to_panes_started_after_it_is_set() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let script = "for i in $(seq -w 0 29); do echo L$i; done; sleep 30";
    client.spawn(pty("before", "sh", &["-c", script])).await.unwrap();
    logs_until(&mut client, "before", "L29").await;
    set(&mut client, "history-limit", "3").await.unwrap();
    client.spawn(pty("after", "sh", &["-c", script])).await.unwrap();
    logs_until(&mut client, "after", "L29").await;

    let (before, _) = client.logs("before", None, true, None).await.unwrap();
    let (after, _) = client.logs("after", None, true, None).await.unwrap();
    assert!(before.contains("L00"), "default scrollback keeps the first line: {before}");
    assert!(
        !after.contains("L03") && after.contains("L04") && after.contains("L29"),
        "three history lines (L04-L06), then the screen: {after}"
    );
}

#[tokio::test]
async fn monitor_options_flag_windows_with_no_terminal_attached() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(window(Some("main"), pty("main-pane", "cat", &[]), true))
        .await
        .unwrap();
    set(&mut client, "monitor-activity", "off").await.unwrap();
    set(&mut client, "monitor-bell", "off").await.unwrap();
    let noise = "sleep 0.2; printf 'hi\\a'; sleep 30";
    client
        .new_window(window(Some("quiet"), pty("quiet-pane", "sh", &["-c", noise]), false))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let before = tabs(&mut client).await;
    assert!(!before[1].has_activity && !before[1].has_bell, "{before:?}");

    set(&mut client, "monitor-activity", "on").await.unwrap();
    set(&mut client, "monitor-bell", "on").await.unwrap();
    client
        .new_window(window(Some("noisy"), pty("noisy-pane", "sh", &["-c", noise]), false))
        .await
        .unwrap();
    let flagged = tabs_until(&mut client, |t| {
        t.get(2).is_some_and(|w| w.has_activity && w.has_bell)
    })
    .await;
    assert!(flagged[2].has_activity && flagged[2].has_bell, "{flagged:?}");
}

#[tokio::test]
async fn automatic_rename_off_keeps_the_name_a_window_was_created_with() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    set(&mut client, "automatic-rename", "off").await.unwrap();
    client
        .new_window(window(
            None,
            pty("auto-pane", "sh", &["-c", "exec sleep 30"]),
            true,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(tabs(&mut client).await[0].name, "sh");

    set(&mut client, "automatic-rename", "on").await.unwrap();
    let renamed = tabs_until(&mut client, |t| t[0].name == "sleep").await;
    assert_eq!(renamed[0].name, "sleep");
}

#[tokio::test]
async fn synchronized_input_is_a_window_flag_that_moves_with_its_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .new_window(window(Some("first"), task("a", "cat", &[]), true))
        .await
        .unwrap();
    client
        .new_window(window(Some("second"), task("b", "cat", &[]), true))
        .await
        .unwrap();
    client.set_synchronized(Some(1), true).await.unwrap();
    let flags: Vec<bool> = tabs(&mut client).await.iter().map(|t| t.synchronized).collect();
    assert_eq!(flags, [false, true]);

    client.swap_window(0, 1).await.unwrap();
    let moved: Vec<(String, bool)> = tabs(&mut client)
        .await
        .iter()
        .map(|t| (t.name.clone(), t.synchronized))
        .collect();
    assert_eq!(
        moved,
        [("second".to_string(), true), ("first".to_string(), false)]
    );
    assert!(client.set_synchronized(Some(9), true).await.is_err());
}

#[tokio::test]
async fn default_shell_command_and_terminal_apply_to_panes_started_after_them() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    set(&mut client, "default-terminal", "screen-256color")
        .await
        .unwrap();
    set(&mut client, "default-shell", "/bin/sh").await.unwrap();
    let shell = client
        .spawn(SpawnOpts {
            name: "shell".into(),
            pty: true,
            ..SpawnOpts::default()
        })
        .await
        .unwrap();
    assert_eq!(shell.command, "/bin/sh");

    set(&mut client, "default-command", "printf 'READY %s' \"$TERM\"; sleep 30")
        .await
        .unwrap();
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
