use std::path::Path;
use std::time::{Duration, Instant};

use super::support::{assert_refused, connect, fresh, start, task};
use mpx::client::Client;
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
