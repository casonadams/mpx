//! Paste buffers, `capture-pane`, and the session environment (`set-environment`,
//! `show-environment`). The buffers are the daemon's, which the CLI shares.
use super::model::{BufferCommand, Command, EnvCommand};
use super::opts::{Operands, Spec, with_opts};
use crate::protocol::{EnvChange, PasteBufferOpts};

const SET_BUFFER: Spec = Spec::until_operand(&["-a"], &["-b", "-n"]);
const BUFFER_OF: Spec = Spec::flags(&[], &["-b"]);
const SAVE_BUFFER: Spec = Spec::flags(&["-a"], &["-b"]);
const PASTE_BUFFER: Spec = Spec::flags(&["-d", "-p", "-r"], &["-b", "-s", "-t"]);
const LIST_BUFFERS: Spec = Spec::flags(&[], &["-F"]);
const CAPTURE_PANE: Spec = Spec::flags(&["-p", "-e", "-J"], &["-t", "-S", "-E", "-b"]);
const SET_ENVIRONMENT: Spec = Spec::until_operand(&["-g", "-u", "-r"], &["-t"]);
const SHOW_ENVIRONMENT: Spec = Spec::flags(&["-g"], &["-t"]);

/// `list-buffers [-F format]`.
pub(super) fn list_buffers(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, LIST_BUFFERS, Operands::Refuse, |o| {
        Ok(BufferCommand::ListBuffers {
            format: o.string("-F"),
        })
    })
    .map(Command::Buffer)
}

/// `set-buffer [-a] [-b name] [-n new] [data]`: stores data, or renames a buffer with `-n`.
pub(super) fn set_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SET_BUFFER, Operands::Allow, |o| {
        let rename = o.string("-n");
        let data = (!o.rest.is_empty()).then(|| o.rest.join(" "));
        match (rename.is_some(), data.is_some()) {
            (true, true) => Err(format!("{name}: -n takes no data")),
            (false, false) => Err(format!("{name}: no data given")),
            _ => Ok(BufferCommand::SetBuffer {
                name: o.string("-b"),
                append: o.has("-a"),
                rename,
                data,
            }),
        }
    })
    .map(Command::Buffer)
}

/// `show-buffer` and `delete-buffer`: an optional `-b name` and no operands.
pub(super) fn named_buffer(
    name: &str,
    args: &[String],
    make: fn(Option<String>) -> BufferCommand,
) -> Result<Command, String> {
    with_opts(name, args, BUFFER_OF, Operands::Refuse, |o| {
        Ok(make(o.string("-b")))
    })
    .map(Command::Buffer)
}

/// `load-buffer [-b name] path`.
pub(super) fn load_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, BUFFER_OF, Operands::Allow, |o| {
        Ok(BufferCommand::LoadBuffer {
            name: o.string("-b"),
            path: single_path(name, &o.rest)?,
        })
    })
    .map(Command::Buffer)
}

/// `save-buffer [-a] [-b name] path`.
pub(super) fn save_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SAVE_BUFFER, Operands::Allow, |o| {
        Ok(BufferCommand::SaveBuffer {
            name: o.string("-b"),
            append: o.has("-a"),
            path: single_path(name, &o.rest)?,
        })
    })
    .map(Command::Buffer)
}

/// The one path operand of `load-buffer` and `save-buffer`.
fn single_path(name: &str, rest: &[String]) -> Result<String, String> {
    match rest {
        [path] => Ok(path.clone()),
        [] => Err(format!("{name}: need a path")),
        _ => Err(format!("{name}: too many arguments")),
    }
}

/// `paste-buffer [-dpr] [-s sep] [-b name] [-t pane]`.
pub(super) fn paste_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, PASTE_BUFFER, Operands::Refuse, |o| {
        Ok(BufferCommand::PasteBuffer {
            opts: PasteBufferOpts {
                name: o.string("-b"),
                pane: o.string("-t").unwrap_or_default(),
                delete: o.has("-d"),
                bracketed: o.has("-p"),
                raw: o.has("-r"),
                separator: o.string("-s"),
            },
        })
    })
    .map(Command::Buffer)
}

/// `capture-pane [-p] [-e] [-J] [-S line] [-E line] [-b name] [-t pane]`.
pub(super) fn capture_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, CAPTURE_PANE, Operands::Refuse, |o| {
        Ok(BufferCommand::CapturePane {
            target: o.string("-t"),
            print: o.has("-p"),
            buffer: o.string("-b"),
            spec: crate::cli::capture_spec(o.value("-S"), o.value("-E"), o.has("-e"), o.has("-J"))?,
        })
    })
    .map(Command::Buffer)
}

/// `set-environment [-gur] name [value]`: `-u` unsets, `-r` removes the variable.
pub(super) fn set_environment(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SET_ENVIRONMENT, Operands::Allow, |o| {
        if o.value("-t").is_some() {
            return Err(format!("{name}: -t is not supported at the prompt"));
        }
        let (var, value) = match o.rest.split_first() {
            Some((var, [])) => (var.clone(), None),
            Some((var, rest)) => (var.clone(), Some(rest.join(" "))),
            None => return Err(format!("{name}: need a variable name")),
        };
        let change = match (o.has("-u"), o.has("-r"), value) {
            (true, true, _) => return Err(format!("{name}: -u and -r conflict")),
            (true, false, None) => EnvChange::Unset,
            (false, true, None) => EnvChange::Remove,
            (true, false, Some(_)) | (false, true, Some(_)) => {
                return Err(format!("{name}: -u and -r take no value"));
            }
            (false, false, Some(value)) => EnvChange::Set(value),
            (false, false, None) => return Err(format!("{name}: no value given for {var}")),
        };
        Ok(EnvCommand::SetEnvironment { name: var, change })
    })
    .map(Command::Environment)
}

/// `show-environment [-g] [name]`.
pub(super) fn show_environment(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SHOW_ENVIRONMENT, Operands::Allow, |o| {
        if o.value("-t").is_some() {
            return Err(format!("{name}: -t is not supported at the prompt"));
        }
        let var = match o.rest.as_slice() {
            [] => None,
            [var] => Some(var.clone()),
            _ => return Err(format!("{name}: too many arguments")),
        };
        Ok(EnvCommand::ShowEnvironment { name: var })
    })
    .map(Command::Environment)
}
