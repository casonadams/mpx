//! Pane commands: splitting, selecting, marking, resizing, swapping, joining, respawning and
//! laying out panes.
use super::model::{Command, JoinOpts, PaneCommand};
use super::opts::{Operands, Spec, TARGET, direction_flag, with_opts};
use crate::protocol::LayoutPreset;

const SPLIT: Spec = Spec::until_operand(&["-h", "-d", "-Z"], &["-c"]);
const SELECT_PANE: Spec = Spec::flags(
    &["-L", "-R", "-U", "-D", "-m", "-M", "-Z", "-d", "-e"],
    &["-t"],
);
const RESIZE_PANE: Spec = Spec::flags(&["-L", "-R", "-U", "-D", "-Z"], &["-t"]);
const SWAP_PANE: Spec = Spec::flags(&["-U", "-D", "-Z"], &["-s", "-t"]);
const JOIN_PANE: Spec = Spec::flags(&["-d", "-h", "-v"], &["-s", "-t"]);
const RESPAWN_PANE: Spec = Spec::until_operand(&["-k"], &["-c", "-t"]);
const LAST_PANE: Spec = Spec::flags(&["-Z"], &[]);

/// `split-window` (stacked) and `vsplit` (`vertical_alias`: side by side) with `-h` for side by
/// side; the words after the flags are the command.
pub(super) fn split(name: &str, args: &[String], vertical_alias: bool) -> Result<Command, String> {
    with_opts(name, args, SPLIT, Operands::Allow, |o| {
        Ok(Command::Pane(PaneCommand::SplitWindow {
            side_by_side: vertical_alias || o.has("-h"),
            cwd: o.string("-c"),
            detach: o.has("-d"),
            keep_zoom: o.has("-Z"),
            argv: o.rest,
        }))
    })
}

pub(super) fn kill_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, TARGET, Operands::Refuse, |o| {
        Ok(Command::Pane(PaneCommand::KillPane {
            target: o.string("-t"),
        }))
    })
}

pub(super) fn select_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SELECT_PANE, Operands::Refuse, |o| {
        let keep_zoom = o.has("-Z");
        if o.has("-M") {
            return Ok(Command::Pane(PaneCommand::ClearMark));
        }
        if o.has("-d") || o.has("-e") {
            return Ok(Command::Pane(PaneCommand::InputOff {
                target: o.string("-t"),
                off: o.has("-d"),
            }));
        }
        if o.has("-m") {
            return Ok(Command::Pane(PaneCommand::MarkPane {
                target: o.string("-t"),
            }));
        }
        if let Some(side) = direction_flag(&o) {
            return Ok(Command::Pane(PaneCommand::SelectPaneDir {
                side,
                keep_zoom,
            }));
        }
        let target = o
            .value("-t")
            .ok_or_else(|| format!("{name}: need -t <target>, -m/-M, or -L/-R/-U/-D"))?;
        Ok(Command::Pane(PaneCommand::SelectPane {
            target: target.to_string(),
            keep_zoom,
        }))
    })
}

pub(super) fn mark_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SELECT_PANE, Operands::Refuse, |o| {
        if o.has("-M") {
            Ok(Command::Pane(PaneCommand::ClearMark))
        } else {
            Ok(Command::Pane(PaneCommand::MarkPane {
                target: o.string("-t"),
            }))
        }
    })
}

/// `resize-pane -L|-R|-U|-D [cells]`, or `-Z` to zoom the active pane.
pub(super) fn resize_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, RESIZE_PANE, Operands::Allow, |o| {
        if o.has("-Z") {
            return if o.value("-t").is_some() {
                Err(format!("{name}: -Z does not take -t"))
            } else {
                Ok(Command::Zoom)
            };
        }
        let side = direction_flag(&o).ok_or_else(|| format!("{name}: need -L/-R/-U/-D or -Z"))?;
        let cells = match o.rest.first() {
            None => 5,
            Some(raw) => raw
                .parse::<u16>()
                .map_err(|_| format!("{name}: invalid cell count '{raw}'"))?,
        };
        Ok(Command::Pane(PaneCommand::ResizePane { side, cells }))
    })
}

/// `swap-pane -U|-D` swaps with a neighbour; otherwise `-s` and `-t` name the panes to swap.
pub(super) fn swap_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, SWAP_PANE, Operands::Refuse, |o| {
        let keep_zoom = o.has("-Z");
        if o.has("-D") || o.has("-U") {
            return Ok(Command::Pane(PaneCommand::SwapPane {
                next: o.has("-D"),
                keep_zoom,
            }));
        }
        Ok(Command::Pane(PaneCommand::SwapPaneMarked {
            source: o.string("-s"),
            target: o.string("-t"),
            keep_zoom,
        }))
    })
}

/// `select-layout [-E | preset]`.
pub(super) fn select_layout(name: &str, args: &[String]) -> Result<Command, String> {
    let preset = match args {
        [flag] if flag == "-E" => return Ok(Command::Pane(PaneCommand::SpreadLayout)),
        [] => None,
        [one] => Some(LayoutPreset::parse(one).ok_or_else(|| format!("unknown layout '{one}'"))?),
        _ => return Err(format!("{name}: too many arguments")),
    };
    Ok(Command::Pane(PaneCommand::SelectLayout { preset }))
}

/// `join-pane [-dh] [-s src] [-t dst]`.
pub(super) fn join_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, JOIN_PANE, Operands::Refuse, |o| {
        Ok(Command::Pane(PaneCommand::JoinPane(JoinOpts {
            src: o.string("-s"),
            dst: o.string("-t"),
            side_by_side: o.has("-h"),
            detach: o.has("-d"),
        })))
    })
}

/// `respawn-pane [-k] [-c dir] [-t target] [command...]`.
pub(super) fn respawn_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, RESPAWN_PANE, Operands::Allow, |o| {
        Ok(Command::Pane(PaneCommand::RespawnPane {
            target: o.string("-t"),
            kill: o.has("-k"),
            cwd: o.string("-c"),
            command: o.rest,
        }))
    })
}

pub(super) fn last_pane(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, LAST_PANE, Operands::Refuse, |o| {
        Ok(Command::Pane(PaneCommand::LastPane {
            keep_zoom: o.has("-Z"),
        }))
    })
}
