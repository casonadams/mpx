use mpx::tui::command::{Command, PopupCommand, parse};
use mpx::tui::mouse_menu::MouseMenu;

fn pane(input_off: bool, marked: bool, zoomed: bool) -> MouseMenu {
    MouseMenu::Pane {
        input_off,
        marked,
        zoomed,
    }
}

#[test]
fn every_mouse_menu_is_a_display_menu_command() {
    let menus = [
        pane(false, false, false),
        pane(true, true, true),
        MouseMenu::Window {
            label: "1:main".into(),
        },
        MouseMenu::Session {
            name: "work".into(),
        },
        MouseMenu::Border,
    ];
    for menu in menus {
        let line = menu.command_line(3, 4);
        let cmd = parse(&line).unwrap_or_else(|e| panic!("`{line}` should parse: {e}"));
        assert!(
            matches!(cmd, Command::Popup(PopupCommand::Menu { .. })),
            "`{line}` is not a menu"
        );
    }
}

#[test]
fn menus_open_at_the_pointer() {
    let line = MouseMenu::Border.command_line(12, 7);
    assert!(line.starts_with("display-menu -x 12 -y 7 "), "{line}");
}

#[test]
fn pane_menu_labels_follow_the_pane_state() {
    let off = pane(true, true, true).command_line(0, 0);
    assert!(
        off.contains("\"Enable Input\" \"i\" \"select-pane -e\""),
        "{off}"
    );
    assert!(off.contains("\"Unmark\" \"m\""), "{off}");
    assert!(off.contains("\"Unzoom\" \"z\""), "{off}");

    let on = pane(false, false, false).command_line(0, 0);
    assert!(
        on.contains("\"Disable Input\" \"i\" \"select-pane -d\""),
        "{on}"
    );
    assert!(on.contains("\"Mark\" \"m\""), "{on}");
    assert!(on.contains("\"Zoom\" \"z\""), "{on}");
}

#[test]
fn menu_titles_keep_quotes_and_format_signs_literal() {
    let line = MouseMenu::Session {
        name: "a\"b#c".into(),
    }
    .command_line(0, 0);
    let cmd = parse(&line).unwrap_or_else(|e| panic!("`{line}` should parse: {e}"));
    assert!(
        matches!(cmd, Command::Popup(PopupCommand::Menu { .. })),
        "{line}"
    );
    assert!(line.contains("-T \"a'b##c\""), "{line}");
}
