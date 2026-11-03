use std::path::Path;
use std::time::{Duration, Instant};

use super::support::{assert_refused, connect, fresh, start, start_with, task};
use mpx::client::Client;
use mpx::config::Config;
use mpx::layout::WindowTab;
use mpx::protocol::{ChannelOp, NewWindowOpts, Request, Response, ScriptRequest};

/// Waits up to two seconds for `path` to appear, as hook commands run in the background.
async fn appears(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Sends a request that answers `Ok`, or the daemon's error.
async fn send(client: &mut Client, req: Request) -> Result<(), String> {
    client.call_ok("request", req).await
}

/// The commands of every hook, in order.
async fn show_hooks(client: &mut Client) -> Vec<(String, String)> {
    match client.call(Request::Script(ScriptRequest::ShowHooks)).await.unwrap() {
        Response::Hooks(hooks) => hooks,
        other => panic!("show-hooks answered {other:?}"),
    }
}

fn set_hook(name: &str, command: Option<&str>, append: bool) -> Request {
    Request::Script(ScriptRequest::SetHook {
        name: name.to_string(),
        command: command.map(str::to_string),
        append,
    })
}

fn wait_for(channel: &str, op: ChannelOp) -> Request {
    Request::Script(ScriptRequest::WaitFor {
        channel: channel.to_string(),
        op,
    })
}

async fn new_window(client: &mut Client, name: &str) {
    client
        .new_window(NewWindowOpts {
            name: None,
            spawn: task(name, "sleep", &["30"]),
            focus: true,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn set_hook_lists_its_commands_in_order() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    send(&mut client, set_hook("after-new-window", Some("run-shell first"), false))
        .await
        .unwrap();
    send(&mut client, set_hook("after-new-window", Some("run-shell second"), true))
        .await
        .unwrap();
    send(&mut client, set_hook("pane-exited", Some("run-shell third"), false))
        .await
        .unwrap();
    let hooks = show_hooks(&mut client).await;
    assert_eq!(
        hooks,
        vec![
            ("after-new-window".to_string(), "run-shell first".to_string()),
            ("after-new-window".to_string(), "run-shell second".to_string()),
            ("pane-exited".to_string(), "run-shell third".to_string()),
        ]
    );
}

#[tokio::test]
async fn set_hook_without_a_command_removes_the_hook() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    send(&mut client, set_hook("pane-exited", Some("run-shell x"), false))
        .await
        .unwrap();
    send(&mut client, set_hook("pane-exited", None, false))
        .await
        .unwrap();
    assert!(show_hooks(&mut client).await.is_empty());
}

#[tokio::test]
async fn unknown_hook_names_and_bad_commands_are_refused() {
    let (_daemon, mut client) = fresh().await;
    let unknown = send(&mut client, set_hook("no-such-hook", Some("run-shell x"), false))
        .await
        .unwrap_err();
    assert!(unknown.contains("unknown hook"), "{unknown}");
    let bad = send(&mut client, set_hook("pane-exited", Some("nosuchcommand"), false))
        .await
        .unwrap_err();
    assert!(bad.contains("unknown command"), "{bad}");
}

#[tokio::test]
async fn after_new_window_runs_its_hook_with_the_hook_variables() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let command = format!(
        "run-shell 'printf %s #{{hook}} > {}'",
        marker.display()
    );
    send(&mut client, set_hook("after-new-window", Some(&command), false))
        .await
        .unwrap();
    new_window(&mut client, "hooked").await;
    assert!(appears(&marker).await, "hook did not run");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "after-new-window");
}

#[tokio::test]
async fn if_shell_in_a_hook_picks_the_branch_by_exit_status() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let yes = dir.path().join("yes");
    let no = dir.path().join("no");
    let command = format!(
        "if-shell 'false' 'run-shell touch {}' 'run-shell touch {}'",
        yes.display(),
        no.display()
    );
    send(&mut client, set_hook("after-new-window", Some(&command), false))
        .await
        .unwrap();
    new_window(&mut client, "branch").await;
    assert!(appears(&no).await, "else branch did not run");
    assert!(!yes.exists());
}

#[tokio::test]
async fn run_command_runs_if_shell_and_the_branch_it_picks() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let yes = dir.path().join("yes");
    let no = dir.path().join("no");
    let line = format!(
        "if-shell 'test -d {}' 'run-shell touch {}' 'run-shell touch {}'",
        dir.path().display(),
        yes.display(),
        no.display()
    );
    send(&mut client, Request::Script(ScriptRequest::RunCommand { line }))
        .await
        .unwrap();
    assert!(appears(&yes).await, "the then branch did not run");
    assert!(!no.exists());
}

#[tokio::test]
async fn run_command_refuses_a_line_that_does_not_parse() {
    let (_daemon, mut client) = fresh().await;
    assert_refused(
        send(
            &mut client,
            Request::Script(ScriptRequest::RunCommand {
                line: "if-shell true nosuchcommand".to_string(),
            }),
        )
        .await,
        "unknown command",
    );
}

#[tokio::test]
async fn run_shell_prints_its_output_and_status() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let shell = client
        .run_shell("echo hello; exit 3", None, false, None)
        .await
        .unwrap()
        .expect("a foreground run answers");
    assert_eq!(shell.output, "hello\n");
    assert_eq!(shell.status, Some(3));
}

#[tokio::test]
async fn run_shell_in_the_background_answers_at_once() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("background");
    let answer = client
        .run_shell(&format!("touch {}", marker.display()), None, true, None)
        .await
        .unwrap();
    assert!(answer.is_none());
    assert!(appears(&marker).await);
}

#[tokio::test]
async fn run_shell_delay_waits_before_running() {
    let (_daemon, mut client) = fresh().await;
    let started = Instant::now();
    client
        .run_shell("true", None, false, Some(300))
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(250));
}

#[tokio::test]
async fn wait_for_returns_once_the_channel_is_signalled() {
    let daemon = start().await;
    let endpoint = daemon.endpoint.clone();
    let waiter = tokio::spawn(async move {
        let mut client = connect(&endpoint).await;
        send(&mut client, wait_for("ready", ChannelOp::Wait)).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut signaller = connect(&daemon.endpoint).await;
    send(&mut signaller, wait_for("ready", ChannelOp::Signal))
        .await
        .unwrap();
    let waited = tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("the waiter returns")
        .unwrap();
    assert!(waited.is_ok());
}

#[tokio::test]
async fn a_signal_with_no_waiter_is_kept_for_the_next_wait() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    send(&mut client, wait_for("early", ChannelOp::Signal))
        .await
        .unwrap();
    let waited = tokio::time::timeout(
        Duration::from_secs(2),
        send(&mut client, wait_for("early", ChannelOp::Wait)),
    )
    .await
    .expect("the kept signal lets the wait through");
    assert!(waited.is_ok());
}

#[tokio::test]
async fn unlock_of_an_unlocked_channel_is_an_error() {
    let (_daemon, mut client) = fresh().await;
    assert_refused(send(&mut client, wait_for("lockname", ChannelOp::Unlock)).await, "not locked");
}

#[tokio::test]
async fn a_second_lock_waits_until_the_first_is_released() {
    let daemon = start().await;
    let mut holder = connect(&daemon.endpoint).await;
    send(&mut holder, wait_for("gate", ChannelOp::Lock))
        .await
        .unwrap();
    let endpoint = daemon.endpoint.clone();
    let taker = tokio::spawn(async move {
        let mut client = connect(&endpoint).await;
        send(&mut client, wait_for("gate", ChannelOp::Lock)).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!taker.is_finished(), "the second lock must wait");
    send(&mut holder, wait_for("gate", ChannelOp::Unlock))
        .await
        .unwrap();
    let taken = tokio::time::timeout(Duration::from_secs(2), taker)
        .await
        .expect("the second lock is granted")
        .unwrap();
    assert!(taken.is_ok());
}

/// Runs `line` the way a hook runs a command: the daemon executes it, as `mpx if-shell` does too.
async fn run_line(client: &mut Client, line: &str) -> Result<(), String> {
    send(
        client,
        Request::Script(ScriptRequest::RunCommand {
            line: line.to_string(),
        }),
    )
    .await
}

/// A window named `name` running `sleep`, which keeps its name.
async fn named_window(client: &mut Client, name: &str) {
    client
        .new_window(NewWindowOpts {
            name: Some(name.to_string()),
            spawn: task(name, "sleep", &["30"]),
            focus: true,
        })
        .await
        .unwrap();
}

/// The session's windows, in order.
async fn tabs(client: &mut Client) -> Vec<WindowTab> {
    client.layout(80, 24).await.unwrap().2
}

#[tokio::test]
async fn if_shell_from_the_cli_sets_a_buffer() {
    let (_daemon, mut client) = fresh().await;
    run_line(&mut client, "if-shell true 'set-buffer -b c yes'")
        .await
        .unwrap();
    assert_eq!(client.get_buffer(Some("c")).await.unwrap().data, "yes");
}

#[tokio::test]
async fn buffer_commands_append_rename_and_delete() {
    let (_daemon, mut client) = fresh().await;
    run_line(&mut client, "set-buffer -b a one").await.unwrap();
    run_line(&mut client, "set-buffer -a -b a two")
        .await
        .unwrap();
    assert_eq!(client.get_buffer(Some("a")).await.unwrap().data, "onetwo");
    run_line(&mut client, "set-buffer -b a -n b").await.unwrap();
    assert!(client.get_buffer(Some("a")).await.is_err());
    assert_eq!(client.get_buffer(Some("b")).await.unwrap().data, "onetwo");
    run_line(&mut client, "delete-buffer -b b").await.unwrap();
    assert!(client.get_buffer(Some("b")).await.is_err());
}

#[tokio::test]
async fn load_and_save_buffer_move_a_file_through_the_daemon() {
    let (_daemon, mut client) = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.txt");
    let output = dir.path().join("out.txt");
    std::fs::write(&input, "loaded").unwrap();
    run_line(
        &mut client,
        &format!("load-buffer -b f {}", input.display()),
    )
    .await
    .unwrap();
    run_line(
        &mut client,
        &format!("save-buffer -b f {}", output.display()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "loaded");
}

#[tokio::test]
async fn load_buffer_of_a_missing_file_is_an_error() {
    let (_daemon, mut client) = fresh().await;
    let err = run_line(&mut client, "load-buffer -b f /no/such/file/anywhere")
        .await
        .unwrap_err();
    assert!(err.contains("load-buffer"), "{err}");
}

#[tokio::test]
async fn set_and_show_environment_run_in_a_hook() {
    let (_daemon, mut client) = fresh().await;
    run_line(&mut client, "set-environment MPX_HOOK_VAR yes")
        .await
        .unwrap();
    let entries = client.show_environment(Some("MPX_HOOK_VAR")).await.unwrap();
    assert_eq!(entries[0].value.as_deref(), Some("yes"));
    run_line(&mut client, "show-environment MPX_HOOK_VAR")
        .await
        .unwrap();
    assert!(
        run_line(&mut client, "show-environment MPX_NOT_SET")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rename_session_runs_in_a_hook() {
    let (_daemon, mut client) = fresh().await;
    run_line(&mut client, "rename-session renamed-by-hook")
        .await
        .unwrap();
    assert_eq!(client.info().await.unwrap().name, "renamed-by-hook");
}

#[tokio::test]
async fn window_commands_swap_move_and_select_by_name() {
    let (_daemon, mut client) = fresh().await;
    named_window(&mut client, "alpha").await;
    named_window(&mut client, "beta").await;
    run_line(&mut client, "swap-window -s 0 -t 1")
        .await
        .unwrap();
    let names: Vec<String> = tabs(&mut client)
        .await
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["beta", "alpha"]);
    run_line(&mut client, "select-window -t alpha")
        .await
        .unwrap();
    assert!(
        tabs(&mut client)
            .await
            .iter()
            .any(|t| t.name == "alpha" && t.is_active)
    );
    run_line(&mut client, "move-window -t 0").await.unwrap();
    let moved = tabs(&mut client).await;
    assert!(
        moved
            .iter()
            .any(|t| t.index == 0 && t.name == "alpha" && t.is_active)
    );
}

#[tokio::test]
async fn select_window_by_a_name_it_does_not_have_is_an_error() {
    let (_daemon, mut client) = fresh().await;
    named_window(&mut client, "alpha").await;
    let err = run_line(&mut client, "select-window -t nosuch")
        .await
        .unwrap_err();
    assert!(err.contains("can't find window: nosuch"), "{err}");
}

#[tokio::test]
async fn zoom_and_synchronize_act_on_the_active_window() {
    let (_daemon, mut client) = fresh().await;
    named_window(&mut client, "zoomed").await;
    run_line(&mut client, "resize-pane -Z").await.unwrap();
    assert!(
        tabs(&mut client)
            .await
            .iter()
            .any(|t| t.is_active && t.zoomed)
    );
    run_line(&mut client, "set-option synchronize-panes on")
        .await
        .unwrap();
    assert!(
        tabs(&mut client)
            .await
            .iter()
            .any(|t| t.is_active && t.synchronized)
    );
}

#[tokio::test]
async fn send_keys_reaches_the_pane_and_capture_stores_its_screen() {
    let (_daemon, mut client) = fresh().await;
    client.spawn(task("typist", "cat", &[])).await.unwrap();
    run_line(&mut client, "send-keys -t typist hello Enter")
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !client.screen("typist").await.unwrap().contains("hello") {
        assert!(Instant::now() < deadline, "the keys never reached the pane");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    run_line(&mut client, "capture-pane -t typist -b shot")
        .await
        .unwrap();
    assert!(
        client
            .get_buffer(Some("shot"))
            .await
            .unwrap()
            .data
            .contains("hello")
    );
}

#[tokio::test]
async fn commands_that_need_the_terminal_are_refused_in_a_hook() {
    let (_daemon, mut client) = fresh().await;
    for line in [
        "copy-mode",
        "select-pane -L",
        "resize-pane -L",
        "display-message hi",
        "show-hooks",
        "detach-client",
        "display-popup true",
    ] {
        let err = run_line(&mut client, line).await.unwrap_err();
        assert!(
            err.contains("command not available in hooks"),
            "{line}: {err}"
        );
    }
}

#[tokio::test]
async fn window_linked_and_unlinked_fire_when_a_window_comes_and_goes() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let linked = dir.path().join("linked");
    let unlinked = dir.path().join("unlinked");
    send(
        &mut client,
        set_hook(
            "window-linked",
            Some(&format!("run-shell 'touch {}'", linked.display())),
            false,
        ),
    )
    .await
    .unwrap();
    send(
        &mut client,
        set_hook(
            "window-unlinked",
            Some(&format!("run-shell 'touch {}'", unlinked.display())),
            false,
        ),
    )
    .await
    .unwrap();
    new_window(&mut client, "linked").await;
    assert!(appears(&linked).await, "window-linked did not run");
    client.kill_window(None).await.unwrap();
    assert!(appears(&unlinked).await, "window-unlinked did not run");
}

#[tokio::test]
async fn session_closed_runs_before_the_session_shuts_down() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let closed = dir.path().join("closed");
    send(
        &mut client,
        set_hook(
            "session-closed",
            Some(&format!("run-shell 'touch {}'", closed.display())),
            false,
        ),
    )
    .await
    .unwrap();
    client.shutdown().await.unwrap();
    assert!(appears(&closed).await, "session-closed did not run");
}

#[tokio::test]
async fn alert_silence_fires_when_a_background_window_goes_quiet() {
    let mut config = Config::default();
    config.window.monitor_silence = 1;
    let daemon = start_with(config).await;
    let mut client = connect(&daemon.endpoint).await;
    // Pane output reaches the session (and so the silence timers) only while a subscriber is attached.
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let watcher = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = watcher
            .stream_logs_into((None, None), false, tx, |event| event)
            .await;
    });
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let dir = tempfile::tempdir().unwrap();
    let silent = dir.path().join("silent");
    send(
        &mut client,
        set_hook(
            "alert-silence",
            Some(&format!("run-shell 'touch {}'", silent.display())),
            false,
        ),
    )
    .await
    .unwrap();
    client
        .new_window(NewWindowOpts {
            name: Some("quiet".to_string()),
            spawn: task("quiet", "sh", &["-c", "echo hi; sleep 30"]),
            focus: true,
        })
        .await
        .unwrap();
    new_window(&mut client, "front").await;
    assert!(appears(&silent).await, "alert-silence did not run");
}
