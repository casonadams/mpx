//! The `:` command prompt: commands run from typed lines, and the prompt's line editing.
use crossterm::event::{Event, KeyCode};

use mpx::config::Config;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, wait_for, window};

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
    h.command(&echo_line("vsplit")).await;
    assert_eq!(h.nodes().await, [MAIN, "pane-2"]);
    wait_for(&mut h.probe, "pane-2", "READY").await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn rename_prompt_acts_on_the_daemon() {
    let mut h = start().await;
    h.frame().await;
    assert_eq!(h.command_windows("rename-window logs").await, ["logs"]);
}

#[tokio::test]
#[cfg(unix)]
async fn send_keys_prompt_acts_on_the_daemon() {
    let mut h = start().await;
    h.frame().await;
    h.command_then_output("send-keys hello Enter", MAIN, "hello\r").await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn select_pane_prompt_focuses_the_named_pane() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");

    h.command("select-pane -t main").await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
}

#[tokio::test]
async fn paste_prompt_writes_text_into_the_target_pane() {
    let mut h = start().await;
    h.frame().await;
    h.command("paste -t main hello world").await;
    wait_for(&mut h.probe, MAIN, "hello world").await;
}

#[tokio::test]
async fn kill_pane_prompt_with_a_target_needs_no_confirmation() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;

    h.command("kill-pane -t pane-2").await;
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
    h.command("bogus").await;
    let message = h.app.live_message().expect("error message").to_string();
    assert!(message.contains("bogus"), "{message}");
    assert!(h.frame().await.contains("bogus"));
}

#[tokio::test]
async fn up_recalls_the_previous_command_line() {
    let mut h = start().await;
    h.frame().await;
    h.command("rename-window first").await;
    h.prefixed_char(':').await;
    h.key(KeyCode::Up).await;
    assert!(h.frame().await.contains(":rename-window first"));
    assert!(matches!(h.app.mode, Mode::Prompt(_)));
}

#[tokio::test]
async fn empty_prompt_submits_nothing() {
    let mut h = start().await;
    h.frame().await;
    h.command("").await;
    assert!(h.mode_is_normal());
    assert!(h.app.live_message().is_none());
    assert_eq!(h.windows().await, [MAIN]);
}

fn active_window(h: &Harness) -> String {
    h.app
        .layout
        .tabs
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.name.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn templated_prompt_asks_for_its_text_and_runs_the_filled_template() {
    let mut h = start().await;
    h.frame().await;
    h.command("command-prompt -p 'window name' \"rename-window -- '%%'\"")
        .await;
    assert!(matches!(&h.app.mode, Mode::Prompt(run) if run.line.label() == "window name"));
    h.type_str("logs").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await, ["logs"]);
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn comma_asks_for_the_window_name_with_the_current_one_filled_in() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(',').await;
    assert!(matches!(&h.app.mode, Mode::Prompt(run) if run.line.text() == MAIN));
    for _ in MAIN.chars() {
        h.key(KeyCode::Backspace).await;
    }
    h.type_str("logs").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await, ["logs"]);
}

#[tokio::test]
async fn quote_binding_completes_window_names_as_its_target() {
    let mut h = start().await;
    h.frame().await;
    window(&mut h.probe, "other", ECHO).await;
    h.frame().await;
    assert_eq!(active_window(&h), "other");
    h.prefixed_char('\'').await;
    h.type_str("ma").await;
    h.key(KeyCode::Tab).await;
    h.key(KeyCode::Enter).await;
    h.frame().await;
    assert_eq!(active_window(&h), MAIN);
}

#[tokio::test]
async fn single_key_prompt_answers_with_one_key_and_no_enter() {
    let mut h = start().await;
    h.frame().await;
    h.command("command-prompt -1 -p key \"rename-window '%%'\"")
        .await;
    h.key(KeyCode::Char('k')).await;
    assert_eq!(h.windows().await, ["k"]);
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn confirm_before_runs_its_command_only_on_y() {
    let mut h = start().await;
    h.frame().await;
    let line = "confirm-before -p 'rename?' \"rename-window gone\"";
    h.command(line).await;
    assert!(matches!(&h.app.mode, Mode::Confirm(question, _) if question.as_str() == "rename?"));
    h.key(KeyCode::Char('n')).await;
    assert_eq!(h.windows().await, [MAIN]);

    h.command(line).await;
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.windows().await, ["gone"]);
}
