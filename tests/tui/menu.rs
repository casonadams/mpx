use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::tui::menu::{Menu, MenuAction, MenuItem};
use mpx::tui::placement::Offset;

fn item(name: &str, key: &str, line: &str) -> MenuItem {
    MenuItem {
        name: name.into(),
        disabled: false,
        key: Some(key.into()),
        line: Some(line.into()),
    }
}

fn disabled(name: &str) -> MenuItem {
    MenuItem {
        disabled: true,
        ..item(name, "o", "kill-pane")
    }
}

fn separator() -> MenuItem {
    MenuItem {
        name: String::new(),
        disabled: false,
        key: None,
        line: None,
    }
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn menu(items: Vec<MenuItem>) -> Menu {
    Menu::new("t".into(), items, Offset::Centre, Offset::Centre, (80, 24))
}

#[test]
fn the_first_selectable_item_starts_selected() {
    let m = menu(vec![separator(), disabled("Off"), item("Zoom", "z", "resize-pane -Z")]);
    assert_eq!(m.selected, 2);
}

#[test]
fn arrows_skip_separators_and_disabled_items_and_stop_at_the_ends() {
    let mut m = menu(vec![
        item("A", "a", "a"),
        separator(),
        disabled("Off"),
        item("B", "b", "b"),
    ]);
    m.move_by(1);
    assert_eq!(m.selected, 3);
    m.move_by(1);
    assert_eq!(m.selected, 3, "stops at the last item");
    m.move_by(-5);
    assert_eq!(m.selected, 0, "stops at the first item");
}

#[test]
fn an_item_key_runs_its_command() {
    let mut m = menu(vec![item("A", "a", "a"), item("Zoom", "z", "resize-pane -Z")]);
    assert_eq!(
        m.handle_key(press(KeyCode::Char('z'))),
        MenuAction::Run("resize-pane -Z".into())
    );
}

#[test]
fn enter_runs_the_selected_item_and_q_or_esc_close_the_menu() {
    let mut m = menu(vec![item("Kill", "x", "kill-pane")]);
    assert_eq!(m.handle_key(press(KeyCode::Enter)), MenuAction::Run("kill-pane".into()));
    assert_eq!(m.handle_key(press(KeyCode::Char('q'))), MenuAction::Close);
    assert_eq!(m.handle_key(press(KeyCode::Esc)), MenuAction::Close);
}

#[test]
fn a_disabled_item_is_never_run_by_its_key() {
    let mut m = menu(vec![item("A", "a", "a"), disabled("Off")]);
    assert_eq!(m.handle_key(press(KeyCode::Char('o'))), MenuAction::Stay);
    assert_eq!(m.selected, 0);
}

#[test]
fn a_menu_is_drawn_inside_the_screen() {
    let m = menu(vec![item("A", "a", "a"), item("B", "b", "b")]);
    assert!(m.rect.x + m.rect.width <= 80 && m.rect.y + m.rect.height <= 24);
}
