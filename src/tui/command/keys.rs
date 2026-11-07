//! Key input: binding and unbinding keys, sending keys and pasting text into panes, and entering
//! copy mode.
use super::model::{Command, CopyModeOpts};
use super::opts::{Operands, Opts, Spec, with_opts};
use crate::tui::keymap::Table;

const BIND: Spec = Spec::until_operand(&["-n", "-r"], &["-T", "-N"]);
const UNBIND: Spec = Spec::flags(&["-n", "-a"], &["-T"]);
const SEND_KEYS: Spec = Spec::until_operand(&["-X"], &["-t", "-N"]);
const PASTE: Spec = Spec::until_operand(&["--submit"], &["-t"]);
const COPY_MODE: Spec = Spec::flags(&["-e", "-H", "-M", "-q", "-u"], &["-t"]);

/// `bind-key [-n] [-r] [-T table] key command...`.
pub(super) fn bind_key(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, BIND, Operands::Allow, |o| {
        let table = key_table(name, &o)?;
        let (key, command) = o
            .rest
            .split_first()
            .ok_or_else(|| format!("{name}: need a key and a command"))?;
        if command.is_empty() {
            return Err(format!("{name}: need a command for key {key}"));
        }
        if table.is_copy() && !is_copy_command(command) {
            return Err(format!(
                "{name}: {} takes only send-keys -X commands",
                table.name()
            ));
        }
        Ok(Command::Bind {
            table,
            key: key.clone(),
            line: crate::cmdline::command_line(command),
        })
    })
}

/// `unbind-key [-n] [-a] [-T table] key`: a bind with an empty command line.
pub(super) fn unbind_key(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, UNBIND, Operands::Allow, |o| {
        let table = key_table(name, &o)?;
        let key = o
            .rest
            .first()
            .ok_or_else(|| format!("{name}: need a key"))?;
        Ok(Command::Bind {
            table,
            key: key.clone(),
            line: String::new(),
        })
    })
}

/// `send-keys [-t pane] keys...`, or `send-keys -X [-N count] [-t pane] command` for copy mode.
pub(super) fn send_keys(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SEND_KEYS, Operands::Allow, |o| {
        let target = o.string("-t");
        if o.has("-X") {
            let (command, command_args) = o
                .rest
                .split_first()
                .ok_or_else(|| format!("{name}: -X needs a copy-mode command"))?;
            let count = match o.value("-N") {
                None => 1,
                Some(raw) => raw
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{name}: invalid repeat count '{raw}'"))?,
            };
            let action = crate::tui::copy::parse_copy_command(command, command_args)?;
            return Ok(Command::CopyCommand {
                target,
                action,
                count,
            });
        }
        if o.value("-N").is_some() {
            return Err(format!("{name}: -N needs -X"));
        }
        if o.rest.is_empty() {
            return Err(format!("{name}: no keys given"));
        }
        Ok(Command::SendKeys {
            target,
            keys: o.rest,
        })
    })
}

/// `paste [-t pane] [--submit] text...`.
pub(super) fn paste(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, PASTE, Operands::Allow, |o| {
        Ok(Command::Paste {
            target: o.string("-t"),
            text: o.rest.join(" "),
            submit: o.has("--submit"),
        })
    })
}

/// `copy-mode [-eHMqu] [-t pane]`. `-H` (hide position) and `-M` (mouse) are accepted; mpx has no
/// position indicator to hide and mouse drags always start copy mode.
pub(super) fn copy_mode(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, COPY_MODE, Operands::Refuse, |o| {
        Ok(Command::CopyMode(CopyModeOpts {
            target: o.string("-t"),
            page_up: o.has("-u"),
            exit_at_bottom: o.has("-e"),
            quit: o.has("-q"),
        }))
    })
}

/// The key table `-T` names, else the root table with `-n`, else the prefix table.
fn key_table(name: &str, o: &Opts) -> Result<Table, String> {
    match (o.value("-T"), o.has("-n")) {
        (Some(table), _) => Table::from_name(table)
            .ok_or_else(|| format!("{name}: key table '{table}' is not supported")),
        (None, true) => Ok(Table::Root),
        (None, false) => Ok(Table::Prefix),
    }
}

/// Whether a command line is `send-keys -X ...`, the only kind a copy-mode table binds.
fn is_copy_command(command: &[String]) -> bool {
    matches!(command, [name, flag, ..] if name == "send-keys" && flag == "-X")
}
