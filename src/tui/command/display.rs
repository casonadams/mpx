//! Display and prompt commands: the status-row message, panes numbering, the clock, the tree and
//! palette overlays, `command-prompt` and `confirm-before`.
use super::model::{Command, PaletteTab, PromptSpec};
use super::opts::{Operands, Spec, TARGET, with_opts};
use super::{Scope, parse_scoped_in};
use crate::tui::prompt::PromptKind;

const DISPLAY_MESSAGE: Spec = Spec::until_operand(&["-p"], &[]);
const DISPLAY_PANES: Spec = Spec::flags(&[], &["-d"]);
/// `choose-tree` flags: `-s` (sessions) and `-w` (windows) pick what the tree opens on, `-Z` zooms.
const CHOOSE_TREE: Spec = Spec::flags(&["-s", "-w", "-Z"], &[]);
const COMMAND_PROMPT: Spec =
    Spec::until_operand(&["-1", "-b", "-F", "-k", "-N"], &["-I", "-p", "-T", "-t"]);
const CONFIRM: Spec = Spec::until_operand(&["-b", "-y"], &["-p", "-t"]);

/// `display-message [-p] [format]`.
pub(super) fn display_message(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, DISPLAY_MESSAGE, Operands::Allow, |o| {
        Ok(Command::DisplayMessage {
            format: (!o.rest.is_empty()).then(|| o.rest.join(" ")),
        })
    })
}

/// `display-panes [-d ms]`.
pub(super) fn display_panes(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, DISPLAY_PANES, Operands::Refuse, |o| {
        let duration_ms = o
            .value("-d")
            .map(|ms| {
                ms.parse::<u64>()
                    .map_err(|_| format!("{name}: bad duration '{ms}'"))
            })
            .transpose()?;
        Ok(Command::DisplayPanes { duration_ms })
    })
}

/// `clock-mode [-t pane]`.
pub(super) fn clock_mode(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, TARGET, Operands::Refuse, |o| {
        Ok(Command::ClockMode {
            target: o.string("-t"),
        })
    })
}

/// `choose-tree [-swZ]`: the tree overlay; the flags are accepted, the overlay opens the same way.
pub(super) fn choose_tree(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, CHOOSE_TREE, Operands::Refuse, |_| {
        Ok(Command::OpenTree)
    })
}

/// `palette [commands|sessions|help]`.
pub(super) fn palette(name: &str, args: &[String]) -> Result<Command, String> {
    let tab = match args.first().map(String::as_str) {
        None => None,
        Some("commands" | "cmd") => Some(PaletteTab::Commands),
        Some("sessions" | "session" | "tree") => Some(PaletteTab::Sessions),
        Some("help" | "keys") => Some(PaletteTab::Help),
        Some(other) => return Err(format!("{name}: unknown tab '{other}'")),
    };
    Ok(Command::OpenPalette { tab })
}

/// `command-prompt [-1bFkN] [-I initial] [-p prompts] [-T type] [template]`.
pub(super) fn command_prompt(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, COMMAND_PROMPT, Operands::Allow, |o| {
        let kind = match o.value("-T") {
            None | Some("command") => PromptKind::Command,
            Some("search") => PromptKind::Search,
            Some("target") => PromptKind::Target,
            Some("window-target") => PromptKind::WindowTarget,
            Some(other) => return Err(format!("{name}: unknown prompt type '{other}'")),
        };
        Ok(Command::CommandPrompt(PromptSpec {
            prompts: split_list(o.value("-p")),
            initial: split_list(o.value("-I")),
            template: (!o.rest.is_empty()).then(|| o.rest.join(" ")),
            kind,
            single_key: o.has("-1") || o.has("-k"),
            numeric: o.has("-N"),
            expand: o.has("-F"),
        }))
    })
}

/// A comma-separated option value as its parts; an absent option has none.
fn split_list(value: Option<&str>) -> Vec<String> {
    value.map_or_else(Vec::new, |v| v.split(',').map(str::to_string).collect())
}

/// `confirm-before [-y] [-p prompt] command`: runs `command` once the user answers `y`; `-y`
/// runs it at once.
pub(super) fn confirm_before(name: &str, args: &[String], scope: Scope) -> Result<Command, String> {
    with_opts(
        name,
        args,
        CONFIRM,
        Operands::Require("need a command"),
        |o| {
            let line = crate::cmdline::command_line(&o.rest);
            let then = parse_scoped_in(&line, scope)?;
            if o.has("-y") {
                return Ok(then);
            }
            let prompt = match o.value("-p") {
                Some(text) => text.to_string(),
                None => format!("Confirm '{line}'? (y/n)"),
            };
            Ok(Command::Confirm {
                prompt,
                then: Box::new(then),
            })
        },
    )
}
