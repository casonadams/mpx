//! Script commands: `run-shell`, `if-shell`, `wait-for`, hooks and `source-file`.
use super::model::ScriptCommand;
use super::opts::{Operands, Spec, with_opts};
use super::{Scope, parse_scoped_in};
use crate::protocol::{ChannelOp, HOOK_EVENTS};

const RUN_SHELL: Spec = Spec::until_operand(&["-b"], &["-t", "-d"]);
const IF_SHELL: Spec = Spec::until_operand(&["-b", "-F"], &["-t"]);
const WAIT_FOR: Spec = Spec::flags(&["-L", "-S", "-U"], &[]);
const SET_HOOK: Spec = Spec::until_operand(&["-a", "-g", "-R", "-u", "-w"], &["-t"]);
const SOURCE_FILE: Spec = Spec::flags(&["-q"], &[]);

/// `run-shell [-b] [-t pane] [-d seconds] command`: the words after the flags form the command.
pub(super) fn run_shell(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    with_opts(
        name,
        args,
        RUN_SHELL,
        Operands::Require("need a shell command"),
        |o| {
            let delay_ms = o
                .value("-d")
                .map(|raw| parse_delay(name, raw))
                .transpose()?;
            Ok(ScriptCommand::RunShell {
                background: o.has("-b"),
                target: o.string("-t"),
                delay_ms,
                command: o.rest.join(" "),
            })
        },
    )
}

/// A delay in seconds as milliseconds; fractions are allowed.
fn parse_delay(name: &str, raw: &str) -> Result<u64, String> {
    match raw.parse::<f64>() {
        Ok(secs) if secs.is_finite() && secs >= 0.0 => Ok((secs * 1000.0).round() as u64),
        _ => Err(format!("{name}: invalid delay '{raw}'")),
    }
}

/// `if-shell [-bF] [-t pane] shell-command command [command]`.
pub(super) fn if_shell(name: &str, args: &[String], scope: Scope) -> Result<ScriptCommand, String> {
    with_opts(name, args, IF_SHELL, Operands::Allow, |o| {
        let (condition, branches) = match o.rest.as_slice() {
            [condition, branches @ ..] if matches!(branches.len(), 1 | 2) => (condition, branches),
            _ => return Err(format!("{name}: need a condition and one or two commands")),
        };
        let then = parse_scoped_in(&branches[0], scope)?;
        let otherwise = branches
            .get(1)
            .map(|line| parse_scoped_in(line, scope).map(Box::new))
            .transpose()?;
        Ok(ScriptCommand::IfShell {
            background: o.has("-b"),
            format: o.has("-F"),
            target: o.string("-t"),
            condition: condition.clone(),
            then: Box::new(then),
            otherwise,
        })
    })
}

/// `wait-for [-L|-S|-U] channel`: `-S` signals, `-L` locks, `-U` unlocks; with none it waits.
pub(super) fn wait_for(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    with_opts(name, args, WAIT_FOR, Operands::Allow, |o| {
        let op = match (o.has("-L"), o.has("-S"), o.has("-U")) {
            (false, false, false) => ChannelOp::Wait,
            (true, false, false) => ChannelOp::Lock,
            (false, true, false) => ChannelOp::Signal,
            (false, false, true) => ChannelOp::Unlock,
            _ => return Err(format!("{name}: use one of -L, -S or -U")),
        };
        match o.rest.as_slice() {
            [channel] if !channel.is_empty() => Ok(ScriptCommand::WaitFor {
                op,
                channel: channel.clone(),
            }),
            [_] | [] => Err(format!("{name}: need a channel name")),
            _ => Err(format!("{name}: too many arguments")),
        }
    })
}

/// `set-hook [-a] hook command`, `set-hook -u hook`, `set-hook -R hook`. Window and pane hooks
/// (`-w`, `-t`) are refused: a hook applies to the whole session.
pub(super) fn set_hook(name: &str, args: &[String], scope: Scope) -> Result<ScriptCommand, String> {
    with_opts(name, args, SET_HOOK, Operands::Allow, |o| {
        if o.has("-w") || o.value("-t").is_some() {
            return Err(format!(
                "{name}: window and pane hooks are not supported; a hook applies to the whole session"
            ));
        }
        let (hook, commands) = o
            .rest
            .split_first()
            .ok_or_else(|| format!("{name}: need a hook name"))?;
        if !HOOK_EVENTS.contains(&hook.as_str()) {
            return Err(format!("{name}: unknown hook '{hook}'"));
        }
        if o.has("-R") || o.has("-u") {
            if o.has("-R") && o.has("-u") {
                return Err(format!("{name}: use one of -R or -u"));
            }
            if !commands.is_empty() {
                return Err(format!("{name}: -R and -u take no command"));
            }
            return Ok(if o.has("-R") {
                ScriptCommand::RunHook { name: hook.clone() }
            } else {
                ScriptCommand::SetHook {
                    name: hook.clone(),
                    command: None,
                    append: false,
                }
            });
        }
        if commands.is_empty() {
            return Err(format!("{name}: need a command for hook {hook}"));
        }
        let line = crate::cmdline::command_line(commands);
        parse_scoped_in(&line, scope)?;
        Ok(ScriptCommand::SetHook {
            name: hook.clone(),
            command: Some(line),
            append: o.has("-a"),
        })
    })
}

/// `source-file [-q] path...`: the commands of each file, run in turn.
pub(super) fn source_file(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    with_opts(
        name,
        args,
        SOURCE_FILE,
        Operands::Require("need a file"),
        |o| {
            Ok(ScriptCommand::SourceFile {
                quiet: o.has("-q"),
                paths: o.rest,
            })
        },
    )
}
