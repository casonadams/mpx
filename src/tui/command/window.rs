//! Window commands: creating, selecting, moving, linking, resizing, rotating and respawning windows.
use super::Scope;
use super::model::{Command, WindowCommand};
use super::opts::{Operands, Opts, Spec, TARGET, direction_flag, with_opts};
use crate::layout::LinkPlace;
use crate::protocol::{LinkOpts, ResizeOpts, UnlinkOpts, WindowFit, WindowResize};

const NEW_WINDOW: Spec = Spec::until_operand(&[], &["-n", "-c"]);
const ROTATE: Spec = Spec::flags(&["-D", "-U", "-Z"], &[]);
const MOVE_WINDOW: Spec = Spec::flags(&["-r"], &["-t"]);
const SWAP_WINDOW: Spec = Spec::flags(&[], &["-s", "-t"]);
const LINK_WINDOW: Spec = Spec::flags(&["-a", "-b", "-k", "-d"], &["-s", "-t"]);
const UNLINK_WINDOW: Spec = Spec::flags(&["-k"], &["-t"]);
const RESIZE_WINDOW: Spec = Spec::flags(&["-a", "-A", "-L", "-R", "-U", "-D"], &["-t", "-x", "-y"]);
const RESPAWN_WINDOW: Spec = Spec::until_operand(&["-k"], &["-c", "-t"]);
const FIND_WINDOW: Spec = Spec::flags(&["-i", "-N", "-T"], &[]);

pub(super) fn new_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, NEW_WINDOW, Operands::Allow, |o| {
        Ok(Command::Window(WindowCommand::NewWindow {
            name: o.string("-n"),
            cwd: o.string("-c"),
            argv: o.rest,
        }))
    })
}

pub(super) fn kill_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, TARGET, Operands::Refuse, |o| {
        let index = o.value("-t").map(parse_index).transpose()?;
        Ok(Command::Window(WindowCommand::KillWindow { index }))
    })
}

pub(super) fn respawn_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, RESPAWN_WINDOW, Operands::Allow, |o| {
        Ok(Command::Window(WindowCommand::RespawnWindow {
            target: o
                .value("-t")
                .map(|raw| window_index(name, raw))
                .transpose()?,
            kill: o.has("-k"),
            cwd: o.string("-c"),
            command: o.rest,
        }))
    })
}

/// `link-window [-s src] [-t dst] [-a|-b] [-k] [-d]`.
pub(super) fn link_window(name: &str, args: &[String], scope: Scope) -> Result<Command, String> {
    with_opts(name, args, LINK_WINDOW, Operands::Refuse, |o| {
        let (after, before) = (o.has("-a"), o.has("-b"));
        if after && before {
            return Err(format!("{name}: -a and -b cannot be given together"));
        }
        let place = LinkPlace {
            after,
            before,
            kill: o.has("-k"),
            detached: o.has("-d"),
        };
        Ok(Command::Window(WindowCommand::LinkWindow(LinkOpts {
            src: window_flag(&o, "-s", scope)?,
            dst: window_flag(&o, "-t", scope)?,
            place,
        })))
    })
}

/// `unlink-window [-k] [-t index]`.
pub(super) fn unlink_window(name: &str, args: &[String], scope: Scope) -> Result<Command, String> {
    with_opts(name, args, UNLINK_WINDOW, Operands::Refuse, |o| {
        Ok(Command::Window(WindowCommand::UnlinkWindow(UnlinkOpts {
            index: window_flag(&o, "-t", scope)?,
            kill: o.has("-k"),
        })))
    })
}

/// `resize-window [-aA] [-LRUD] [-x W] [-y H] [-t index] [adjustment]`.
pub(super) fn resize_window(name: &str, args: &[String], scope: Scope) -> Result<Command, String> {
    with_opts(name, args, RESIZE_WINDOW, Operands::Allow, |o| {
        let adjustment = match o.rest.as_slice() {
            [] => None,
            [cells] => Some(cells_arg(name, cells)?),
            _ => return Err(format!("{name}: at most one adjustment")),
        };
        let sides = ["-L", "-R", "-U", "-D"].iter().filter(|f| o.has(f)).count();
        if sides > 1 {
            return Err(format!("{name}: give one of -L, -R, -U or -D"));
        }
        if o.has("-a") && o.has("-A") {
            return Err(format!("{name}: -a and -A cannot be given together"));
        }
        let fit = if o.has("-A") {
            Some(WindowFit::Largest)
        } else if o.has("-a") {
            Some(WindowFit::Smallest)
        } else {
            None
        };
        Ok(Command::Window(WindowCommand::ResizeWindow(ResizeOpts {
            index: window_flag(&o, "-t", scope)?,
            resize: WindowResize {
                width: o.value("-x").map(|v| cells_arg(name, v)).transpose()?,
                height: o.value("-y").map(|v| cells_arg(name, v)).transpose()?,
                side: direction_flag(&o),
                cells: adjustment,
                fit,
            },
        })))
    })
}

pub(super) fn rename_window(args: &[String]) -> Result<Command, String> {
    // tmux's `rename-window -- name` ends the options: the name may start with a dash.
    let words = args.strip_prefix(&["--".to_string()]).unwrap_or(args);
    let name = words.join(" ");
    if name.trim().is_empty() {
        return Err("rename-window: missing name".to_string());
    }
    Ok(Command::Window(WindowCommand::RenameWindow { name }))
}

pub(super) fn select_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, TARGET, Operands::Refuse, |o| {
        let raw = o
            .value("-t")
            .ok_or_else(|| format!("{name}: need -t <index>"))?;
        Ok(match parse_index(raw) {
            Ok(index) => Command::Window(WindowCommand::SelectWindow { index }),
            Err(_) => Command::Window(WindowCommand::SelectWindowNamed {
                name: raw.rsplit(':').next().unwrap_or(raw).to_string(),
            }),
        })
    })
}

pub(super) fn rotate_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, ROTATE, Operands::Refuse, |o| {
        Ok(Command::Window(WindowCommand::RotateWindow {
            down: o.has("-D"),
            keep_zoom: o.has("-Z"),
        }))
    })
}

pub(super) fn move_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, MOVE_WINDOW, Operands::Refuse, |o| {
        if o.has("-r") {
            return Ok(Command::Window(WindowCommand::RenumberWindows));
        }
        let raw = o
            .value("-t")
            .ok_or_else(|| format!("{name}: need -t <index> or -r"))?;
        Ok(Command::Window(WindowCommand::MoveWindow {
            to: window_index(name, raw)?,
        }))
    })
}

pub(super) fn swap_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SWAP_WINDOW, Operands::Refuse, |o| {
        let dst = o
            .value("-t")
            .ok_or_else(|| format!("{name}: need -t <index>"))?;
        let src = o.value("-s").map(|s| window_index(name, s)).transpose()?;
        Ok(Command::Window(WindowCommand::SwapWindow {
            src,
            dst: window_index(name, dst)?,
        }))
    })
}

/// `find-window [-iNT] pattern`.
pub(super) fn find_window(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(
        name,
        args,
        FIND_WINDOW,
        Operands::Require("need a pattern"),
        |o| {
            Ok(Command::Window(WindowCommand::FindWindow {
                pattern: o.rest.join(" "),
            }))
        },
    )
}

/// A window index named by flag `flag`, which may carry a session name in `scope`.
fn window_flag(o: &Opts, flag: &str, scope: Scope) -> Result<Option<usize>, String> {
    o.value(flag)
        .map(|t| window_target(t, scope.session))
        .transpose()
}

/// A size or adjustment in cells, a positive number.
fn cells_arg(name: &str, raw: &str) -> Result<u16, String> {
    raw.parse::<u16>()
        .map_err(|_| format!("{name}: not a number of cells: {raw}"))
}

/// A window index for a link or resize flag: `N` or `:N` of the session the prompt runs in, or
/// `session:N` of `session` (`None` when the session is not known here). Any other session is
/// refused: each session runs in its own daemon, so its windows cannot be shared with this one.
pub fn window_target(raw: &str, session: Option<&str>) -> Result<usize, String> {
    let index = match raw.rsplit_once(':') {
        Some((name, _)) if !name.is_empty() && session != Some(name) => {
            return Err(format!(
                "window {raw} is in another session: each session runs in its own daemon, so its windows cannot be shared with this one (see docs/ARCHITECTURE.md)"
            ));
        }
        Some((_, index)) => index,
        None => raw,
    };
    index
        .parse::<usize>()
        .map_err(|_| format!("invalid window index '{raw}'"))
}

/// A window index for a command flag: `N` or `:N`.
fn window_index(name: &str, raw: &str) -> Result<usize, String> {
    parse_index(raw).map_err(|_| format!("{name}: invalid window index '{raw}'"))
}

fn parse_index(s: &str) -> Result<usize, String> {
    s.strip_prefix(':')
        .unwrap_or(s)
        .parse::<usize>()
        .map_err(|_| format!("invalid window index '{s}'"))
}
