//! Copy-mode commands: the `CopyAction` values and the parser that names them.
use super::{Motion, SearchDirection};

/// What a copy command copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyWhat {
    /// The selection; nothing when there is none.
    Selection,
    /// The selection, or the cursor's line when there is none (mpx's vi `y`).
    SelectionOrLine,
    /// The cursor's whole line.
    Line,
    /// From the cursor to the end of its line.
    EndOfLine,
}

/// One copy-mode command, named as tmux's `send-keys -X` names it. The mpx-only names are the
/// ones tmux has no command for: they keep mpx's own key behavior (see `docs/KEYBINDINGS.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyAction {
    /// A move. `cancel_at_bottom` (`cursor-down-and-cancel`) leaves copy mode when the move reaches
    /// the bottom of the live screen.
    Move {
        motion: Motion,
        cancel_at_bottom: bool,
    },
    /// `cancel`: leaves copy mode.
    Cancel,
    /// `clear-selection`.
    ClearSelection,
    /// mpx: clears the selection when there is one, else leaves copy mode (vi `Escape`).
    ClearSelectionOrCancel,
    /// `begin-selection`: a character selection anchored at the cursor.
    BeginSelection,
    /// `select-line`.
    SelectLine,
    /// `select-word`.
    SelectWord,
    /// mpx: starts or clears a character selection (vi `v`, emacs `C-Space`).
    ToggleSelection,
    /// mpx: starts or clears a line selection (vi `V`).
    ToggleLineSelection,
    /// `other-end`: swaps the cursor and the other end of the selection.
    OtherEnd,
    /// `rectangle-toggle` (vi `C-v`): a character selection covers a rectangle instead of a run.
    RectangleToggle,
    /// `set-mark`: marks the cursor's line.
    SetMark,
    /// `jump-to-mark`: the cursor goes to the mark and the mark to the line the cursor left.
    JumpToMark,
    /// `rectangle-on` / `rectangle-off`: sets the rectangle selection on or off, without toggling.
    SetRectangle(bool),
    /// `next-prompt` / `previous-prompt`: the next or previous shell prompt, or with `output` (`-o`)
    /// the next or previous start of command output, as marked by OSC 133.
    JumpToPrompt { forward: bool, output: bool },
    /// `copy-*`: copies `what`; `cancel` leaves copy mode, `clear` drops the selection after
    /// copying, and `pipe` also pipes the text to a shell command.
    Copy {
        what: CopyWhat,
        cancel: bool,
        clear: bool,
        pipe: Option<String>,
    },
    /// mpx: copies the selection and leaves copy mode, but only when a selection exists (emacs
    /// `Enter`).
    CopyIfSelected,
    /// `search-forward` / `search-backward` with their text.
    SearchText {
        direction: SearchDirection,
        query: String,
    },
    /// `search-again` (`reverse`: `search-reverse`) repeats the last search.
    SearchAgain { reverse: bool },
    /// mpx: opens the search prompt (vi `/` and `?`, emacs `C-s` and `C-r`).
    SearchPrompt(SearchDirection),
}

/// Parses one copy-mode command: `name` and the arguments that follow it. Commands tmux has that
/// mpx cannot express (`rectangle-toggle`, the mark commands, ...) are refused by name.
pub fn parse_copy_command(name: &str, args: &[String]) -> Result<CopyAction, String> {
    if let Some(action) = plain_command(name) {
        return Ok(action);
    }
    let pipe = (!args.is_empty()).then(|| args.join(" "));
    if let Some(action) = copy_command(name, pipe.as_deref()) {
        return Ok(action);
    }
    match name {
        "goto-line" => {
            let raw = args.first().map_or("", String::as_str);
            let line = raw
                .parse::<usize>()
                .map_err(|_| format!("send-keys -X goto-line: invalid line '{raw}'"))?;
            Ok(move_action(Motion::GotoLine(line)))
        }
        "jump-forward" | "jump-backward" | "jump-to-forward" | "jump-to-backward" => {
            let ch = args
                .first()
                .and_then(|a| a.chars().next())
                .ok_or_else(|| format!("send-keys -X {name}: needs a character"))?;
            Ok(move_action(Motion::Jump {
                forward: name.ends_with("forward"),
                to: name.starts_with("jump-to"),
                ch,
            }))
        }
        "search-forward" => Ok(CopyAction::SearchText {
            direction: SearchDirection::Down,
            query: required_text(name, args)?,
        }),
        "search-backward" => Ok(CopyAction::SearchText {
            direction: SearchDirection::Up,
            query: required_text(name, args)?,
        }),
        "next-prompt" | "previous-prompt" => Ok(CopyAction::JumpToPrompt {
            forward: name == "next-prompt",
            output: prompt_output_flag(name, args)?,
        }),
        _ => Err(format!("copy-mode command not supported: {name}")),
    }
}

/// The command for a motion key.
fn move_action(motion: Motion) -> CopyAction {
    CopyAction::Move {
        motion,
        cancel_at_bottom: false,
    }
}

/// `-o` (the start of command output) or no argument; any other argument is refused.
fn prompt_output_flag(name: &str, args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "-o" => Ok(true),
        _ => Err(format!("send-keys -X {name}: takes only -o")),
    }
}

/// The arguments joined by spaces; a command that needs text fails without them.
fn required_text(name: &str, args: &[String]) -> Result<String, String> {
    let text = args.join(" ");
    if text.is_empty() {
        Err(format!("send-keys -X {name}: needs an argument"))
    } else {
        Ok(text)
    }
}

/// The commands that take no argument.
fn plain_command(name: &str) -> Option<CopyAction> {
    let action = match name {
        "cursor-left" => move_action(Motion::Left),
        "cursor-right" => move_action(Motion::Right),
        "cursor-up" => move_action(Motion::Up),
        "cursor-down" => move_action(Motion::Down),
        "cursor-down-and-cancel" => CopyAction::Move {
            motion: Motion::Down,
            cancel_at_bottom: true,
        },
        "start-of-line" => move_action(Motion::LineStart),
        "end-of-line" => move_action(Motion::LineEnd),
        "back-to-indentation" => move_action(Motion::BackToIndentation),
        "next-word" => move_action(Motion::NextWord { big: false }),
        "next-space" => move_action(Motion::NextWord { big: true }),
        "previous-word" => move_action(Motion::PreviousWord { big: false }),
        "previous-space" => move_action(Motion::PreviousWord { big: true }),
        "next-word-end" => move_action(Motion::NextWordEnd { big: false }),
        "next-space-end" => move_action(Motion::NextWordEnd { big: true }),
        "forward-four-columns" => move_action(Motion::FourForward),
        "back-four-columns" => move_action(Motion::FourBack),
        "top-line" => move_action(Motion::TopLine),
        "middle-line" => move_action(Motion::MiddleLine),
        "bottom-line" => move_action(Motion::BottomLine),
        "history-top" => move_action(Motion::HistoryTop),
        "history-bottom" => move_action(Motion::HistoryBottom),
        "page-up" => move_action(Motion::PageUp),
        "page-down" => move_action(Motion::PageDown),
        "halfpage-up" => move_action(Motion::HalfPageUp),
        "halfpage-down" => move_action(Motion::HalfPageDown),
        "halfpage-down-and-cancel" => CopyAction::Move {
            motion: Motion::HalfPageDown,
            cancel_at_bottom: true,
        },
        "scroll-up" => move_action(Motion::ScrollUp),
        "scroll-down" => move_action(Motion::ScrollDown),
        "cancel" => CopyAction::Cancel,
        "clear-selection" => CopyAction::ClearSelection,
        "clear-selection-or-cancel" => CopyAction::ClearSelectionOrCancel,
        "begin-selection" => CopyAction::BeginSelection,
        "select-line" => CopyAction::SelectLine,
        "select-word" => CopyAction::SelectWord,
        "toggle-selection" => CopyAction::ToggleSelection,
        "toggle-line-selection" => CopyAction::ToggleLineSelection,
        "other-end" => CopyAction::OtherEnd,
        "rectangle-toggle" => CopyAction::RectangleToggle,
        "set-mark" => CopyAction::SetMark,
        "jump-to-mark" => CopyAction::JumpToMark,
        "rectangle-on" => CopyAction::SetRectangle(true),
        "rectangle-off" => CopyAction::SetRectangle(false),
        "copy-selection-if-any" => CopyAction::CopyIfSelected,
        "search-again" => CopyAction::SearchAgain { reverse: false },
        "search-reverse" => CopyAction::SearchAgain { reverse: true },
        "search-prompt-forward" => CopyAction::SearchPrompt(SearchDirection::Down),
        "search-prompt-backward" => CopyAction::SearchPrompt(SearchDirection::Up),
        _ => return None,
    };
    Some(action)
}

/// The `copy-*` and `copy-pipe*` commands. Only the pipe forms take the piped argument.
fn copy_command(name: &str, pipe: Option<&str>) -> Option<CopyAction> {
    let (what, cancel, clear) = match name {
        "copy-selection" | "copy-pipe" => (CopyWhat::Selection, false, true),
        "copy-selection-no-clear" | "copy-pipe-no-clear" => (CopyWhat::Selection, false, false),
        "copy-selection-and-cancel" | "copy-pipe-and-cancel" => (CopyWhat::Selection, true, true),
        "copy-selection-or-line-and-cancel" => (CopyWhat::SelectionOrLine, true, false),
        "copy-line" | "copy-pipe-line" => (CopyWhat::Line, false, false),
        "copy-line-and-cancel" | "copy-pipe-line-and-cancel" => (CopyWhat::Line, true, false),
        "copy-end-of-line" | "copy-pipe-end-of-line" => (CopyWhat::EndOfLine, false, false),
        "copy-end-of-line-and-cancel" | "copy-pipe-end-of-line-and-cancel" => {
            (CopyWhat::EndOfLine, true, false)
        }
        _ => return None,
    };
    let pipe = if name.starts_with("copy-pipe") {
        pipe.map(str::to_owned)
    } else {
        None
    };
    Some(CopyAction::Copy {
        what,
        cancel,
        clear,
        pipe,
    })
}
