//! The `:` command prompt: commands run from typed lines, and the prompt's line editing.
use crossterm::event::{Event, KeyCode};

use mpx::config::Config;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, wait_for};

fn echo_line(command: &str) -> String {
    format!("{command} sh -c '{ECHO}'")
}

async fn start() -> Harness {
    Harness::start(Config::default(), ECHO).await
}

#[tokio::test]
async fn vsplit_prompt_creates_a_side_by_side_pane() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str(&echo_line("vsplit")).await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.nodes().await, [MAIN, "pane-2"]);
    wait_for(&mut h.probe, "pane-2", "READY").await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn rename_and_send_keys_prompts_act_on_the_daemon() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("rename-window logs").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await, ["logs"]);

    h.prefixed_char(':').await;
    h.type_str("send-keys hello Enter").await;
    h.key(KeyCode::Enter).await;
    wait_for(&mut h.probe, MAIN, "hello\r").await;
}

#[tokio::test]
async fn select_pane_prompt_focuses_the_named_pane() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");

    h.prefixed_char(':').await;
    h.type_str("select-pane -t main").await;
    h.key(KeyCode::Enter).await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
}

#[tokio::test]
async fn paste_prompt_writes_text_into_the_target_pane() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("paste -t main hello world").await;
    h.key(KeyCode::Enter).await;
    wait_for(&mut h.probe, MAIN, "hello world").await;
}

#[tokio::test]
async fn kill_pane_prompt_with_a_target_needs_no_confirmation() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;

    h.prefixed_char(':').await;
    h.type_str("kill-pane -t pane-2").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.layout_panes().await, [MAIN]);
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn prompt_edits_text_and_escape_cancels_without_running() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.event(Event::Paste("kill-pane".into())).await;
    assert!(h.frame().await.contains(":kill-pane"));
    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());
    assert_eq!(h.nodes().await, [MAIN]);
}

#[tokio::test]
async fn unknown_command_reports_on_the_status_row() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("bogus").await;
    h.key(KeyCode::Enter).await;
    let message = h.app.live_message().expect("error message").to_string();
    assert!(message.contains("bogus"), "{message}");
    assert!(h.frame().await.contains("bogus"));
}

#[tokio::test]
async fn up_recalls_the_previous_command_line() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("rename-window first").await;
    h.key(KeyCode::Enter).await;
    h.prefixed_char(':').await;
    h.key(KeyCode::Up).await;
    assert!(h.frame().await.contains(":rename-window first"));
    assert!(matches!(h.app.mode, Mode::Prompt(_)));
}

#[tokio::test]
async fn empty_prompt_submits_nothing() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.key(KeyCode::Enter).await;
    assert!(h.mode_is_normal());
    assert!(h.app.live_message().is_none());
    assert_eq!(h.windows().await, [MAIN]);
}
