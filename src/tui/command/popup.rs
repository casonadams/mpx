//! Popup and menu commands: `display-popup` runs a command in a box over the panes, `display-menu`
//! shows a list of command lines.
use super::model::{Command, PopupCommand};
use super::opts::{Operands, Spec, with_opts};
use super::parse;
use crate::keys::KeySpec;
use crate::tui::menu::MenuItem;
use crate::tui::placement::{Extent, Offset};
use crate::tui::popup::PopupOpts;

const DISPLAY_POPUP: Spec =
    Spec::until_operand(&["-E"], &["-w", "-h", "-x", "-y", "-d", "-T", "-t"]);
const DISPLAY_MENU: Spec = Spec::until_operand(&[], &["-T", "-x", "-y"]);

/// `display-popup [-E] [-w W] [-h H] [-x X] [-y Y] [-d dir] [-T title] [-t pane] [command]`.
pub(super) fn display_popup(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, DISPLAY_POPUP, Operands::Allow, |o| {
        let mut opts = PopupOpts {
            close_on_exit: o.has("-E"),
            title: o.string("-T"),
            cwd: o.string("-d"),
            target: o.string("-t"),
            command: (!o.rest.is_empty()).then(|| crate::cmdline::command_line(&o.rest)),
            ..PopupOpts::default()
        };
        if let Some(raw) = o.value("-w") {
            opts.width = Extent::parse(raw).map_err(|e| format!("{name}: -w: {e}"))?;
        }
        if let Some(raw) = o.value("-h") {
            opts.height = Extent::parse(raw).map_err(|e| format!("{name}: -h: {e}"))?;
        }
        if let Some(raw) = o.value("-x") {
            opts.x = Offset::parse(raw).map_err(|e| format!("{name}: -x: {e}"))?;
        }
        if let Some(raw) = o.value("-y") {
            opts.y = Offset::parse(raw).map_err(|e| format!("{name}: -y: {e}"))?;
        }
        Ok(Command::Popup(PopupCommand::Display(opts)))
    })
}

/// `display-menu [-T title] [-x X] [-y Y] name key command ...`: items come three words at a time.
pub(super) fn display_menu(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, DISPLAY_MENU, Operands::Allow, |o| {
        if o.rest.is_empty() || o.rest.len() % 3 != 0 {
            return Err(format!(
                "{name}: items are a name, a key and a command, three words each"
            ));
        }
        let items = o
            .rest
            .chunks(3)
            .map(|item| menu_item(name, item))
            .collect::<Result<Vec<_>, _>>()?;
        let place = |flag: &str| {
            o.value(flag)
                .map_or(Ok(Offset::Centre), Offset::parse)
                .map_err(|e| format!("{name}: {flag}: {e}"))
        };
        Ok(Command::Popup(PopupCommand::Menu {
            title: o.string("-T"),
            x: place("-x")?,
            y: place("-y")?,
            items,
        }))
    })
}

/// One `display-menu` item from its name, key and command. A name starting with `-` is shown
/// disabled; an empty name is a separator.
fn menu_item(name: &str, item: &[String]) -> Result<MenuItem, String> {
    let [label, key, line] = item else {
        return Err(format!("{name}: items are a name, a key and a command"));
    };
    if label.is_empty() {
        return Ok(MenuItem {
            name: String::new(),
            disabled: false,
            key: None,
            line: None,
        });
    }
    let (disabled, label) = match label.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, label.as_str()),
    };
    let key = (!key.is_empty()).then(|| key.clone());
    if let Some(spec) = &key {
        KeySpec::parse(spec).map_err(|e| format!("{name}: {label}: key {spec}: {e}"))?;
    }
    let line = (!line.is_empty()).then(|| line.clone());
    if let Some(text) = &line {
        parse(text).map_err(|e| format!("{name}: {label}: {e}"))?;
    }
    Ok(MenuItem {
        name: label.to_string(),
        disabled,
        key,
        line,
    })
}
