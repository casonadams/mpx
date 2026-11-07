//! Session commands (switching and renaming) and the options that `set-option` and
//! `show-options` read and write, including the command aliases of `set -s command-alias`.
use super::model::{Command, ScriptCommand, SessionCommand};
use super::opts::{Operands, Spec, TARGET, parse_opts, with_opts};

const SWITCH: Spec = Spec::flags(&["-l", "-n", "-p"], &["-c", "-t"]);
const SET_OPTION: Spec = Spec::flags(&["-g", "-w", "-s", "-p", "-q", "-u"], &[]);
const SHOW_OPTIONS: Spec = Spec::flags(&["-g", "-w", "-s", "-q", "-v"], &[]);

/// `switch-client [-l | -n | -p] [-t session]`: switches this client to the named session, or to the
/// last, next or previous one.
pub(super) fn switch_client(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SWITCH)?;
    if o.value("-c").is_some() {
        return Err(format!(
            "{name}: -c runs from the command line (mpx switch-client -c CLIENT ...)"
        ));
    }
    let step = [
        ("-l", SessionCommand::LastSession),
        ("-n", SessionCommand::NextSession),
        ("-p", SessionCommand::PrevSession),
    ]
    .into_iter()
    .find(|(flag, _)| o.has(flag))
    .map(|(_, cmd)| cmd);
    if let Some(cmd) = step {
        o.no_rest(name)?;
        return Ok(Command::Session(cmd));
    }
    let session = o.target_or_first(name, "need -t <session>")?;
    Ok(Command::Session(SessionCommand::SwitchSession {
        name: session,
    }))
}

pub(super) fn rename_session(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, TARGET, Operands::Allow, |o| {
        Ok(Command::Session(SessionCommand::RenameSession {
            name: o.target_or_first(name, "need new name")?,
        }))
    })
}

/// `set-option [-gwsqu] name [value]`; `set -s command-alias` sets an alias, and
/// `synchronize-panes` makes typed input go to every pane of the window (`-g`: every window).
pub(super) fn set_option(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SET_OPTION, Operands::Allow, |o| {
        if o.has("-p") {
            return Err(format!("{name}: mpx has no pane options"));
        }
        let unset = o.has("-u");
        if o.has("-s")
            && o.rest
                .first()
                .is_some_and(|option| is_command_alias(option))
        {
            return alias_option(name, &o.rest[1..], unset).map(Command::Script);
        }
        if o.has("-s") {
            return Err(format!(
                "{name}: mpx has no server options to set at run time"
            ));
        }
        let (option, value) = match o.rest.as_slice() {
            [option] if unset => (option, None),
            [option, value] if !unset => (option, Some(value.clone())),
            [] => return Err(format!("{name}: need an option")),
            [_] => return Err(format!("{name}: need a value")),
            _ => return Err(format!("{name}: too many arguments")),
        };
        if option.as_str() == "synchronize-panes" {
            let on = match &value {
                Some(v) => crate::config::parse_switch(v)
                    .map_err(|_| format!("{name}: synchronize-panes takes on or off"))?,
                None => false,
            };
            return Ok(Command::SetSynchronize {
                all: o.has("-g"),
                on,
            });
        }
        Ok(Command::SetOption {
            name: option.clone(),
            value,
            unset,
            quiet: o.has("-q"),
        })
    })
}

/// `show-options [-gwsqv] [name]`.
pub(super) fn show_options(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SHOW_OPTIONS, Operands::Allow, |o| {
        if o.rest.len() > 1 {
            return Err(format!("{name}: too many arguments"));
        }
        Ok(Command::ShowOptions {
            name: o.rest.first().cloned(),
            value_only: o.has("-v"),
            quiet: o.has("-q"),
        })
    })
}

/// Whether a server option names the command aliases (`command-alias`, optionally `[N]`).
fn is_command_alias(option: &str) -> bool {
    option.split('[').next() == Some("command-alias")
}

/// `set -s command-alias[N] name=command` (the index is accepted: aliases are looked up by name),
/// or `set -su command-alias name` to remove one.
fn alias_option(name: &str, rest: &[String], unset: bool) -> Result<ScriptCommand, String> {
    match (rest, unset) {
        ([alias], true) => Ok(ScriptCommand::SetAlias {
            name: alias.clone(),
            command: None,
        }),
        ([pair], false) => match pair.split_once('=') {
            Some((alias, command)) if !alias.is_empty() && !command.is_empty() => {
                Ok(ScriptCommand::SetAlias {
                    name: alias.to_string(),
                    command: Some(command.to_string()),
                })
            }
            _ => Err(format!("{name}: command-alias needs name=command")),
        },
        _ => Err(format!(
            "{name}: command-alias needs name=command, or -u name"
        )),
    }
}
