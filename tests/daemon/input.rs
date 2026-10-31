use super::support::{logs_until, pty, started_with, task};
use mpx::client::Client;
use mpx::protocol::{SpawnOpts, WaitCondition};

/// Waits (bounded) until the pane `name` prints READY.
async fn wait_ready(client: &mut Client, name: &str) {
    client
        .wait(
            name,
            WaitCondition::Match {
                pattern: "READY".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();
}

/// Waits until `needle` shows up in the output of `name`, then kills `name` and returns that
/// output.
async fn output_then_kill(client: &mut Client, name: &str, needle: &str) -> String {
    let logs = logs_until(client, name, needle).await;
    assert!(logs.contains(needle), "logs: {logs}");
    client.kill(name).await.unwrap();
    logs
}

/// Starts `spawn`, sends `keys` to it, and returns its output once `needle` shows up.
async fn keys_output(spawn: SpawnOpts, keys: &[&str], literal: bool, needle: &str) -> String {
    let name = spawn.name.clone();
    let (_daemon, mut client) = started_with(spawn).await;
    let keys = keys.iter().map(|k| k.to_string()).collect();
    client.send_keys(&name, keys, literal).await.unwrap();
    output_then_kill(&mut client, &name, needle).await
}

/// Pastes `text` into a `sh` pane named `name` running `script` (which prints READY first), and
/// checks that the output carries bracketed-paste markers exactly when `bracketed` says so.
async fn paste_is_bracketed_in(
    name: &str,
    script: &str,
    text: &str,
    submit: bool,
    bracketed: bool,
) {
    let (_daemon, mut client) = started_with(pty(name, "sh", &["-c", script])).await;
    wait_ready(&mut client, name).await;
    client.paste(name, text, submit).await.unwrap();
    let logs = output_then_kill(&mut client, name, text).await;
    assert_eq!(logs.contains("200~"), bracketed, "logs: {logs:?}");
}

/// Starts `spawn`, pastes `text` into it, and returns its output once `needle` shows up.
async fn paste_and_expect(spawn: SpawnOpts, text: &str, submit: bool, needle: &str) -> String {
    let name = spawn.name.clone();
    let (_daemon, mut client) = started_with(spawn).await;
    client.paste(&name, text, submit).await.unwrap();
    output_then_kill(&mut client, &name, needle).await
}

#[tokio::test]
async fn literal_send_keys_are_written_as_text() {
    keys_output(task("lit", "cat", &[]), &["lit", "eral", "\n"], true, "literal").await;
}

#[tokio::test]
async fn named_keys_are_encoded_for_the_pty() {
    keys_output(pty("keys", "cat", &[]), &["o", "k", "Enter"], false, "ok").await;
}

#[tokio::test]
async fn paste_is_bracketed_when_the_program_enables_it() {
    paste_is_bracketed_in(
        "bracketed",
        "printf '\\033[?2004hREADY\\n'; cat",
        "pasted_text",
        false,
        true,
    )
    .await;
}

#[tokio::test]
async fn paste_is_plain_without_bracketed_mode_and_submit_presses_enter() {
    paste_is_bracketed_in("plain", "echo READY; cat", "submitted_line", true, false).await;
}

#[tokio::test]
async fn task_paste_submit_appends_newline() {
    paste_and_expect(task("tasked", "cat", &[]), "via_task", true, "via_task\n").await;
}
