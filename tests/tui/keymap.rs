use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::config::Config;
use mpx::protocol::Side;
use mpx::tui::command::Command;
use mpx::tui::keymap::{Table, default_table, resolve};

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn default_keymap() -> mpx::tui::keymap::Keymap {
    resolve(&Config::default()).expect("default keymap resolves")
}

fn command_of(keymap: &mpx::tui::keymap::Keymap, ev: &KeyEvent) -> Option<Command> {
    command_in(keymap, Table::Prefix, ev)
}

fn command_in(
    keymap: &mpx::tui::keymap::Keymap,
    table: Table,
    ev: &KeyEvent,
) -> Option<Command> {
    let binding = keymap.lookup(table, ev)?;
    Some(binding.command.clone())
}

fn alt(c: char) -> KeyEvent {
    let mut ev = key(c);
    ev.modifiers = KeyModifiers::ALT;
    ev
}

#[test]
fn default_table_binds_digits_and_core_keys() {
    let table = default_table();
    let find = |key: &str| {
        table
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, c)| c.as_str())
    };
    assert_eq!(find("c"), Some("new-window"));
    assert_eq!(find("%"), Some("split-window -h"));
    assert_eq!(find("\""), Some("split-window"));
    assert_eq!(find("x"), Some("kill-pane"));
    assert_eq!(find("A"), Some("next-attention"));
    assert_eq!(find("7"), Some("select-window -t 7"));
}

#[test]
fn default_prefix_is_ctrl_b_and_ctrl_a_is_not_a_leader() {
    let keymap = default_keymap();
    assert!(keymap.is_prefix(&ctrl('b')));
    assert!(!keymap.is_prefix(&ctrl('a')));
}

#[test]
fn prefix_comes_from_config() {
    let config = Config {
        prefix: "C-a".to_string(),
        ..Config::default()
    };
    let keymap = resolve(&config).unwrap();
    assert!(keymap.is_prefix(&ctrl('a')));
    assert!(!keymap.is_prefix(&ctrl('b')));
}

#[test]
fn prefix_pressed_twice_sends_the_prefix_key() {
    let keymap = default_keymap();
    assert_eq!(command_of(&keymap, &ctrl('b')), Some(Command::SendPrefix));
}

#[test]
fn default_bindings_resolve_to_commands() {
    let keymap = default_keymap();
    assert_eq!(
        command_of(&keymap, &key('%')),
        Some(Command::SplitWindow {
            side_by_side: true,
            argv: vec![],
            cwd: None,
            detach: false,
        })
    );
    assert_eq!(
        command_of(&keymap, &key('h')),
        Some(Command::SelectPaneDir { side: Side::Left })
    );
    assert_eq!(
        command_of(&keymap, &key('x')),
        Some(Command::KillPane { target: None })
    );
    assert_eq!(
        command_of(&keymap, &key('3')),
        Some(Command::SelectWindow { index: 3 })
    );
    assert_eq!(command_of(&keymap, &key('z')), Some(Command::Zoom));
    assert_eq!(command_of(&keymap, &key('s')), Some(Command::OpenAgents));
    assert_eq!(command_of(&keymap, &key('a')), None);
    assert_eq!(command_of(&keymap, &key('w')), Some(Command::OpenTree));
    assert_eq!(command_of(&keymap, &key('Z')), None);
}

#[test]
fn help_entries_show_key_and_source_command() {
    let entries = default_keymap().help_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.key == "c" && e.command == "new-window")
    );
}

#[test]
fn config_bind_replaces_a_default_key() {
    let mut config = Config::default();
    config
        .bind
        .insert("d".to_string(), "kill-server".to_string());
    let keymap = resolve(&config).unwrap();
    assert!(matches!(
        command_of(&keymap, &key('d')),
        Some(Command::Confirm { .. })
    ));
    assert_eq!(
        keymap
            .bindings()
            .iter()
            .filter(|b| b.key.matches(&key('d')))
            .count(),
        1
    );
}

#[test]
fn config_bind_adds_a_new_key() {
    let mut config = Config::default();
    config.bind.insert("C-x".to_string(), "agents".to_string());
    let keymap = resolve(&config).unwrap();
    assert_eq!(command_of(&keymap, &ctrl('x')), Some(Command::OpenAgents));
}

#[test]
fn invalid_config_bind_is_rejected() {
    let mut config = Config::default();
    config
        .bind
        .insert("y".to_string(), "frobnicate".to_string());
    let err = resolve(&config).unwrap_err();
    assert!(err.contains("bind y"), "{err}");
    assert!(err.contains("unknown command"), "{err}");
}

#[test]
fn invalid_prefix_is_rejected() {
    let config = Config {
        prefix: "Hyper-q".to_string(),
        ..Config::default()
    };
    assert!(resolve(&config).is_err());
}


#[test]
fn alt_j_and_alt_k_step_through_agents_without_the_prefix() {
    let keymap = default_keymap();
    let root = |c| command_in(&keymap, Table::Root, &alt(c));
    assert_eq!(root('j'), Some(Command::StepAgent { next: true }));
    assert_eq!(root('k'), Some(Command::StepAgent { next: false }));
    assert_eq!(command_in(&keymap, Table::Root, &key('j')), None, "plain j reaches the pane");
    assert_eq!(command_of(&keymap, &alt('j')), None, "not a prefix binding");
}

#[test]
fn config_bind_root_rebinds_and_an_empty_command_unbinds() {
    let mut config = Config::default();
    config.bind_root.insert("M-j".to_string(), String::new());
    config.bind_root.insert("M-n".to_string(), "next-agent".to_string());
    config.bind.insert("e".to_string(), " ".to_string());
    let keymap = resolve(&config).unwrap();
    assert_eq!(command_in(&keymap, Table::Root, &alt('j')), None);
    assert_eq!(
        command_in(&keymap, Table::Root, &alt('n')),
        Some(Command::StepAgent { next: true })
    );
    assert_eq!(command_of(&keymap, &key('e')), None, "unbound prefix key");
}

#[cfg(target_os = "macos")]
#[test]
fn option_j_and_option_k_step_through_agents_when_the_terminal_sends_their_characters() {
    let keymap = default_keymap();
    let root = |c| command_in(&keymap, Table::Root, &key(c));
    assert_eq!(root('∆'), Some(Command::StepAgent { next: true }));
    assert_eq!(root('˚'), Some(Command::StepAgent { next: false }));
}
