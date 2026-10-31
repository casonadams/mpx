//! Key bindings set while running (`bind-key`) reach every terminal.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::tui::command::Command;
use mpx::tui::keymap::Table;

use super::support::{Harness, MAIN};

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn bound(h: &Harness, c: char) -> Option<Command> {
    h.app.keymap.lookup(Table::Prefix, &press(c)).map(|b| b.command.clone())
}

#[tokio::test]
async fn a_binding_set_by_another_client_arrives_as_an_event_and_survives_a_reload() {
    let mut h = Harness::idle().await;
    assert!(matches!(bound(&h, 'y'), None));

    h.probe
        .set_binding("prefix:y", Some("kill-window"))
        .await
        .unwrap();
    h.until("the binding", |app| {
        app.keymap.lookup(Table::Prefix, &press('y')).is_some()
    })
    .await;
    assert!(matches!(bound(&h, 'y'), Some(Command::KillWindow { .. })));

    h.probe
        .set_binding("prefix:x", Some(""))
        .await
        .unwrap();
    h.until("the default x removed", |app| {
        app.keymap.lookup(Table::Prefix, &press('x')).is_none()
    })
    .await;

    h.app.apply_bindings().await;
    assert!(bound(&h, 'y').is_some(), "applying again keeps session bindings");
}

#[tokio::test]
async fn typing_bind_key_at_the_prompt_binds_it_here_and_records_it_in_the_session() {
    let mut h = Harness::idle().await;
    h.command("bind-key w split-window -h").await;
    assert!(matches!(bound(&h, 'w'), Some(Command::SplitWindow { side_by_side: true, .. })));
    let recorded = h.probe.get_bindings().await.unwrap();
    assert_eq!(recorded, [("prefix:w".to_string(), "split-window -h".to_string())]);

    h.command("bind-key q nosuchcommand").await;
    assert!(h.app.message.as_ref().is_some_and(|(m, _)| m.contains("unknown command")));
}

#[tokio::test]
async fn chained_commands_run_in_order() {
    let mut h = Harness::idle().await;
    let line = "new-window -n one sh -c cat ; rename-window two";
    assert_eq!(h.command_windows(line).await, [MAIN, "two"]);
}
