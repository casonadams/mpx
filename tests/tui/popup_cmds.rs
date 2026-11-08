use mpx::tui::command::{Command, PopupCommand, parse};
use mpx::tui::placement::{Extent, Offset};

fn popup(line: &str) -> mpx::tui::popup::PopupOpts {
    match parse(line) {
        Ok(Command::Popup(PopupCommand::Display(opts))) => opts,
        other => panic!("`{line}` should be a popup, got {other:?}"),
    }
}

#[test]
fn display_popup_takes_size_position_and_close_on_exit() {
    let opts = popup("display-popup -E -w 40 -h 50% -x R -y 2 -T Log 'tail -f log'");
    assert!(opts.close_on_exit);
    assert_eq!(opts.width, Extent::Cells(40));
    assert_eq!(opts.height, Extent::Percent(50));
    assert_eq!(opts.x, Offset::Far);
    assert_eq!(opts.y, Offset::Cells(2));
    assert_eq!(opts.title.as_deref(), Some("Log"));
    assert_eq!(opts.command.as_deref(), Some("tail -f log"));
}

#[test]
fn display_popup_without_a_command_runs_the_shell_and_defaults_to_eighty_percent() {
    let opts = popup("display-popup");
    assert!(!opts.close_on_exit);
    assert_eq!(opts.command, None);
    assert_eq!(opts.width, Extent::Percent(80));
    assert_eq!(opts.x, Offset::Centre);
}

#[test]
fn display_popup_words_after_the_flags_form_one_command() {
    let opts = popup("display-popup -E htop -d");
    assert_eq!(opts.command.as_deref(), Some("htop -d"));
}

#[test]
fn display_popup_rejects_a_bad_size() {
    assert!(parse("display-popup -w wide").is_err());
    assert!(parse("display-popup -x middle").is_err());
}

#[test]
fn display_menu_takes_items_as_name_key_and_command_triples() {
    match parse("display-menu -T Pick -x R Zoom z 'resize-pane -Z' '' '' '' -Off o 'kill-pane'") {
        Ok(Command::Popup(PopupCommand::Menu { title, x, y, items })) => {
            assert_eq!(title.as_deref(), Some("Pick"));
            assert_eq!(x, Offset::Far);
            assert_eq!(y, Offset::Centre);
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].name, "Zoom");
            assert_eq!(items[0].key.as_deref(), Some("z"));
            assert_eq!(items[0].line.as_deref(), Some("resize-pane -Z"));
            assert!(
                items[1].name.is_empty() && items[1].line.is_none(),
                "a separator"
            );
            assert!(items[2].disabled && items[2].name == "Off");
        }
        other => panic!("expected a menu, got {other:?}"),
    }
}

#[test]
fn display_menu_items_come_in_threes() {
    assert!(parse("display-menu Zoom z").is_err());
    assert!(parse("display-menu Zoom z 'resize-pane -Z' Kill").is_err());
}

#[test]
fn display_menu_rejects_an_item_command_it_cannot_parse() {
    assert!(parse("display-menu Bad b 'no-such-command'").is_err());
}
