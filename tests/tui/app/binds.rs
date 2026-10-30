//! Key bindings and options set while running (`bind-key`, `set-option`) reach every terminal.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::config::Config;
use mpx::protocol::VarKind;
use mpx::tui::command::Command;
use mpx::tui::keymap::Table;

use super::support::{ECHO, Harness};

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn bound(h: &Harness, c: char) -> Option<Command> {
    h.app.keymap.lookup(Table::Prefix, &press(c)).map(|b| b.command.clone())
}

#[tokio::test]
async fn a_binding_set_by_another_client_arrives_as_an_event_and_survives_a_reload() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    assert!(matches!(bound(&h, 'y'), None));

    h.probe
        .set_var(VarKind::Binding, "prefix:y", Some("kill-window"))
        .await
        .unwrap();
    h.until("the binding", |app| {
        app.keymap.lookup(Table::Prefix, &press('y')).is_some()
    })
    .await;
    assert!(matches!(bound(&h, 'y'), Some(Command::KillWindow { .. })));

    h.probe
        .set_var(VarKind::Binding, "prefix:x", Some(""))
        .await
        .unwrap();
    h.until("the default x removed", |app| {
        app.keymap.lookup(Table::Prefix, &press('x')).is_none()
    })
    .await;

    h.app.apply_runtime().await;
    assert!(bound(&h, 'y').is_some(), "applying again keeps session bindings");
}

#[tokio::test]
async fn a_prefix_option_changes_the_prefix_key_and_unsetting_it_restores_the_file() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
    h.probe.set_var(VarKind::Option, "prefix", Some("C-a")).await.unwrap();
    h.until("the new prefix", |app| app.keymap.is_prefix(&ctrl_a)).await;

    h.probe.set_var(VarKind::Option, "prefix", None).await.unwrap();
    h.until("the old prefix", |app| !app.keymap.is_prefix(&ctrl_a)).await;
}

#[tokio::test]
async fn typing_bind_key_at_the_prompt_binds_it_here_and_records_it_in_the_session() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("bind-key w split-window -h").await;
    h.key(KeyCode::Enter).await;
    assert!(matches!(bound(&h, 'w'), Some(Command::SplitWindow { side_by_side: true, .. })));
    let recorded = h.probe.get_vars(VarKind::Binding).await.unwrap();
    assert_eq!(recorded, [("prefix:w".to_string(), "split-window -h".to_string())]);

    h.prefixed_char(':').await;
    h.type_str("bind-key q nosuchcommand").await;
    h.key(KeyCode::Enter).await;
    assert!(h.app.message.as_ref().is_some_and(|(m, _)| m.contains("unknown command")));
}

#[tokio::test]
async fn other_tmux_commands_typed_at_the_prompt_run_through_the_shim() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("set-option -g @theme dark").await;
    h.key(KeyCode::Enter).await;
    let options = h.probe.get_vars(VarKind::Option).await.unwrap();
    assert_eq!(options, [("@theme".to_string(), "dark".to_string())]);

    h.prefixed_char(':').await;
    h.type_str("show-options -gv @theme").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.app.message.as_ref().map(|(m, _)| m.as_str()), Some("dark"));
}

#[tokio::test]
async fn chained_commands_run_in_order() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("new-window -n one sh -c cat ; rename-window two").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await.last().map(String::as_str), Some("two"));
}
