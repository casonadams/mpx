//! Typed TUI commands. Key bindings, the `:` prompt and overlays all produce `Command`s.
//! Targets use `[session:]pane` where pane is a node name or `%id`.
use super::copy::CopyAction;
use super::keymap::Table;
use super::prompt::PromptKind;
use crate::protocol::{ChannelOp, EnvChange, HOOK_EVENTS, LayoutPreset, PasteBufferOpts, Side};
use crate::vt::capture::CaptureSpec;
use std::collections::BTreeMap;

/// The options of `copy-mode`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyModeOpts {
    /// `-t`: the pane; `None` is the active pane.
    pub target: Option<String>,
    /// `-u`: also one page up into the history.
    pub page_up: bool,
    /// `-e`: leave copy mode once scrolling brings the view back to the live screen.
    pub exit_at_bottom: bool,
    /// `-q`: leave copy mode instead of entering it.
    pub quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteTab {
    #[default]
    Commands,
    Sessions,
    Help,
}

/// What a `command-prompt` asks and how its answers are used.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptSpec {
    /// Prompt texts (`-p`), one question each; commas separate them.
    pub prompts: Vec<String>,
    /// Initial text per question (`-I`).
    pub initial: Vec<String>,
    /// Command template with `%%` / `%1`..`%9` placeholders; `None` runs the typed line.
    pub template: Option<String>,
    /// What Tab completes (`-T`).
    pub kind: PromptKind,
    /// The first key typed answers (`-1`, `-k`), with no Enter.
    pub single_key: bool,
    /// Digits only (`-N`).
    pub numeric: bool,
    /// The prompts and initial texts are formats, expanded for the active pane (`-F`).
    pub expand: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The window commands: see `WindowCommand`.
    Window(WindowCommand),
    /// The pane commands: see `PaneCommand`.
    Pane(PaneCommand),
    /// The session commands: see `SessionCommand`.
    Session(SessionCommand),
    /// `set-option [-gwsqu] name [value]`: sets a TUI option for this client; `-u` restores the
    /// default, `-q` ignores an unknown option.
    SetOption {
        name: String,
        value: Option<String>,
        unset: bool,
        quiet: bool,
    },
    /// `show-options [-gwsqv] [name]`: the options and their values in an overlay; with a name and
    /// `-v`, only the value on the status row.
    ShowOptions {
        name: Option<String>,
        value_only: bool,
        quiet: bool,
    },
    /// `display-message [-p] [format]`: the expanded format on the status row; with no format it
    /// describes the current pane.
    DisplayMessage { format: Option<String> },
    SendKeys {
        target: Option<String>,
        keys: Vec<String>,
    },
    Paste {
        target: Option<String>,
        text: String,
        submit: bool,
    },
    Zoom,
    /// `set-option [-g] synchronize-panes on|off`: typed input goes to every pane of the active
    /// window (every window with `-g`).
    SetSynchronize { all: bool, on: bool },
    /// `copy-mode [-eHMqu] [-t pane]`: enters copy mode, or with `quit` leaves it.
    CopyMode(CopyModeOpts),
    /// `send-keys -X command [args]`: runs one copy-mode command in the copy mode of `target`
    /// (the active pane when `None`), `count` times.
    CopyCommand {
        target: Option<String>,
        action: CopyAction,
        count: usize,
    },
    Detach,
    KillServer,
    /// Send the prefix key itself to the active pane.
    SendPrefix,
    /// Re-read the config file into the TUI and the session's daemon.
    ReloadConfig,
    OpenTree,
    OpenHelp,
    OpenPalette { tab: Option<PaletteTab> },
    /// `command-prompt`: one question per `prompts` entry (`-p`), each starting with its `initial`
    /// text (`-I`). With a `template` the answers fill it in (`%%`, `%1`..`%9`); without one the
    /// typed line runs as a command.
    CommandPrompt(PromptSpec),
    Confirm { prompt: String, then: Box<Command> },
    /// Commands run one after the other (`a ; b`), stopping at the first that fails or quits.
    Sequence(Vec<Command>),
    /// `bind-key`: bind `key` in `table`; an empty `line` is `unbind-key`.
    Bind {
        table: Table,
        key: String,
        line: String,
    },
    /// `display-panes [-d ms]`: number each visible pane until a digit picks one, a key is
    /// pressed or the time runs out. `None` takes `display-panes-time`; `Some(0)` waits for a key.
    DisplayPanes { duration_ms: Option<u64> },
    /// `clock-mode [-t pane]`: a large clock fills the pane until a key is pressed.
    ClockMode { target: Option<String> },
    /// `show-messages`: the status-row messages of this session, oldest first.
    ShowMessages,
    /// `choose-buffer`: the paste buffers, newest first; Enter pastes, `d` deletes.
    ChooseBuffer,
    /// Paste buffers, capture and the buffer files: see `BufferCommand`.
    Buffer(BufferCommand),
    /// `set-environment` and `show-environment`: see `EnvCommand`.
    Environment(EnvCommand),
    /// `suspend-client`: stop the process as Ctrl-Z does; the TUI resumes on SIGCONT.
    SuspendClient,
    /// The script commands: `run-shell`, `if-shell`, `wait-for`, hooks, `command-alias` and
    /// `source-file`.
    Script(ScriptCommand),
}

/// The window commands. They act on the active window, or on the window at the index, name or
/// offset they carry.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    NewWindow {
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
    },
    KillWindow { index: Option<usize> },
    RenameWindow { name: String },
    SelectWindow { index: usize },
    /// `select-window -t name`: the window with that name.
    SelectWindowNamed { name: String },
    /// `rotate-window [-D]`: the active window's panes move through its layout slots.
    RotateWindow { down: bool },
    /// `move-window -t index`: moves the active window to that index (`-r` renumbers instead).
    MoveWindow { to: usize },
    /// `move-window -r`: renumbers the windows 0, 1, ... in order.
    RenumberWindows,
    /// `swap-window [-s src] -t dst`: exchanges two windows; `src` defaults to the active one.
    SwapWindow { src: Option<usize>, dst: usize },
    /// `find-window [-iNT] pattern`: the sessions and windows palette filtered by `pattern`.
    FindWindow { pattern: String },
    /// `next-window -a` / `previous-window -a`: the next window with activity or a bell.
    StepAlert { forward: bool },
    NextWindow,
    PrevWindow,
    LastWindow,
}

/// The pane commands. They act on the active pane, or on the pane a `target` names.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneCommand {
    /// `side_by_side`: vertical divider (`:vsplit`, tmux `split-window -h`).
    SplitWindow {
        side_by_side: bool,
        argv: Vec<String>,
        cwd: Option<String>,
        detach: bool,
    },
    KillPane { target: Option<String> },
    SelectPane { target: String },
    SelectPaneDir { side: Side },
    MarkPane { target: Option<String> },
    ClearMark,
    /// `join-pane [-h] [-s src] [-t dst]`: moves `src` (default: the marked pane) next to `dst`
    /// (default: the active pane), side by side with `-h`, else stacked.
    JoinPane {
        src: Option<String>,
        dst: Option<String>,
        side_by_side: bool,
    },
    /// `respawn-pane [-k] [-c dir] [-t target] [command...]`: restarts the pane's process.
    RespawnPane {
        target: Option<String>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    },
    NextPane,
    LastPane,
    ResizePane { side: Side, cells: u16 },
    SwapPane { next: bool },
    SwapPaneMarked {
        source: Option<String>,
        target: Option<String>,
    },
    BreakPane,
    SelectLayout { preset: Option<LayoutPreset> },
    /// `select-layout -E`: spread the panes evenly, keeping their arrangement.
    SpreadLayout,
}

/// The session commands: switching between sessions and renaming the current one.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    SwitchSession { name: String },
    NextSession,
    PrevSession,
    RenameSession { name: String },
}

/// Commands that run scripts, hooks, channels and aliases in the daemon, or read them from files.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptCommand {
    /// `run-shell [-b] [-t pane] [-d seconds] command`: runs `command` through the shell in the
    /// daemon, with formats expanded for the pane. Its output goes to the status row, a message per
    /// line; `-b` does not wait for it.
    RunShell {
        background: bool,
        target: Option<String>,
        delay_ms: Option<u64>,
        command: String,
    },
    /// `if-shell [-bF] [-t pane] shell-command command [command]`: runs `then` when the shell
    /// command exits 0, else `otherwise`. With `-F` the condition is a format, true when non-empty
    /// and not `0`.
    IfShell {
        background: bool,
        format: bool,
        target: Option<String>,
        condition: String,
        then: Box<Command>,
        otherwise: Option<Box<Command>>,
    },
    /// `wait-for [-L|-S|-U] channel`: waits for a signal on, or takes or releases the lock of, a
    /// named channel of the daemon.
    WaitFor {
        op: ChannelOp,
        channel: String,
    },
    /// `set-hook [-a] hook command` sets the commands a hook runs, `-a` after the existing ones;
    /// `set-hook -u hook` removes them (`command` is `None`).
    SetHook {
        name: String,
        command: Option<String>,
        append: bool,
    },
    /// `set-hook -R hook`: runs the commands of a hook now.
    RunHook {
        name: String,
    },
    /// `show-hooks`: every hook command, one line each, in an overlay.
    ShowHooks,
    /// `set -s command-alias name=command` sets an alias; `-u` removes it (`command` is `None`).
    SetAlias {
        name: String,
        command: Option<String>,
    },
    /// `source-file [-q] path...`: runs the commands of each file, one command line per line.
    SourceFile {
        paths: Vec<String>,
        quiet: bool,
    },
}

/// The paste-buffer and capture commands. They act on the daemon's buffers, which the CLI shares.
#[derive(Debug, Clone, PartialEq)]
pub enum BufferCommand {
    /// `list-buffers [-F format]`: the paste buffers, one line each, in an overlay.
    ListBuffers { format: Option<String> },
    /// `set-buffer [-a] [-b name] [-n new] [data]`: stores or renames a paste buffer.
    SetBuffer {
        name: Option<String>,
        append: bool,
        rename: Option<String>,
        data: Option<String>,
    },
    /// `show-buffer [-b name]`: the text of a paste buffer, in an overlay.
    ShowBuffer { name: Option<String> },
    /// `delete-buffer [-b name]`.
    DeleteBuffer { name: Option<String> },
    /// `load-buffer [-b name] path`: a file becomes a paste buffer.
    LoadBuffer { name: Option<String>, path: String },
    /// `save-buffer [-a] [-b name] path`: a paste buffer is written to a file.
    SaveBuffer {
        name: Option<String>,
        append: bool,
        path: String,
    },
    /// `paste-buffer [-d] [-p] [-r] [-s sep] [-b name] [-t pane]`.
    PasteBuffer { opts: PasteBufferOpts },
    /// `capture-pane [-p] [-e] [-J] [-S line] [-E line] [-b name] [-t pane]`.
    CapturePane {
        target: Option<String>,
        print: bool,
        buffer: Option<String>,
        spec: CaptureSpec,
    },
}

/// The session environment commands.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvCommand {
    /// `set-environment [-g] [-u] [-r] name [value]`: a variable of the session environment.
    SetEnvironment { name: String, change: EnvChange },
    /// `show-environment [-g] [name]`: the session environment, in an overlay.
    ShowEnvironment { name: Option<String> },
}

/// Command names and aliases accepted by `parse`; used for `:` completion.
pub const COMMAND_NAMES: &[&str] = &[
    "bind-key",
    "break-pane",
    "capture-pane",
    "choose-buffer",
    "choose-tree",
    "clock-mode",
    "command-palette",
    "command-prompt",
    "confirm-before",
    "copy-mode",
    "delete-buffer",
    "detach",
    "detach-client",
    "display-message",
    "display-panes",
    "find-window",
    "help",
    "if-shell",
    "join-pane",
    "kill-pane",
    "kill-server",
    "kill-window",
    "last-pane",
    "last-window",
    "list-buffers",
    "list-keys",
    "load-buffer",
    "mark-pane",
    "move-window",
    "new",
    "new-window",
    "next-layout",
    "next-pane",
    "next-session",
    "next-window",
    "palette",
    "paste",
    "paste-buffer",
    "prev-session",
    "prev-window",
    "previous-window",
    "q",
    "qa",
    "quit",
    "reload-config",
    "rename",
    "rename-session",
    "rename-window",
    "resize-pane",
    "respawn-pane",
    "rotate-window",
    "run-shell",
    "save-buffer",
    "select-layout",
    "select-pane",
    "select-window",
    "send-keys",
    "send-prefix",
    "session",
    "set-buffer",
    "set-environment",
    "set-hook",
    "set-option",
    "set-window-option",
    "show-buffer",
    "show-environment",
    "show-hooks",
    "show-messages",
    "show-options",
    "show-window-options",
    "source-file",
    "sp",
    "split",
    "split-window",
    "suspend-client",
    "swap-pane",
    "swap-window",
    "switch-client",
    "tabn",
    "tabnew",
    "tabnext",
    "tabp",
    "tabprevious",
    "unbind-key",
    "vs",
    "vsplit",
    "wait-for",
];

/// Parses a command line (tmux command names plus vim aliases such as `vsplit`, `q`), without
/// command aliases.
pub fn parse(line: &str) -> Result<Command, String> {
    parse_in(line, &BTreeMap::new())
}

/// Parses a command line with the command `aliases` of the config (tmux `command-alias`): a
/// command whose name is an alias runs the alias's command line, with its own arguments after it.
pub fn parse_in(line: &str, aliases: &BTreeMap<String, String>) -> Result<Command, String> {
    if line.contains(';') {
        let mut commands = Vec::new();
        for words in crate::cmdline::split_commands(line)? {
            commands.push(parse_words(&expand_alias(words, aliases)?, aliases)?);
        }
        return match commands.len() {
            0 => Err("empty command".to_string()),
            1 => Ok(commands.remove(0)),
            _ => Ok(Command::Sequence(commands)),
        };
    }
    parse_words(&expand_alias(split_words(line)?, aliases)?, aliases)
}

/// The words with an aliased command name replaced by the alias's words. An alias is expanded
/// once, so an alias may name another command but not loop.
fn expand_alias(
    mut words: Vec<String>,
    aliases: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let Some(target) = words.first().and_then(|name| aliases.get(name)) else {
        return Ok(words);
    };
    let mut expanded = split_words(target)?;
    expanded.extend(words.drain(1..));
    Ok(expanded)
}

fn parse_words(words: &[String], aliases: &BTreeMap<String, String>) -> Result<Command, String> {
    let (name, args) = words
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    window_cmd(name, args)
        .or_else(|| pane_cmd(name, args))
        .or_else(|| misc_cmd(name, args, aliases))
        .unwrap_or_else(|| Err(format!("unknown command: {name}")))
}

const BIND: Spec = Spec {
    bools: &["-n", "-r"],
    values: &["-T", "-N"],
    stop_at_arg: true,
};
const UNBIND: Spec = Spec {
    bools: &["-n", "-a"],
    values: &["-T"],
    stop_at_arg: false,
};

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

fn bind_key(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, BIND)?;
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
}

fn unbind_key(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, UNBIND)?;
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
}

/// Splits a command line into words: whitespace separates, `'..'` is literal, `".."` honours
/// `\"` and `\\`, and a bare backslash escapes the next character.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' && matches!(chars.peek(), Some(&('"' | '\\'))) => {
                cur.push(chars.next().unwrap_or(c));
            }
            Some(_) => cur.push(c),
            None if c.is_whitespace() => flush_word(&mut words, &mut cur, &mut in_word),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c == '\\' => {
                in_word = true;
                cur.push(chars.next().unwrap_or('\\'));
            }
            None => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".to_string());
    }
    flush_word(&mut words, &mut cur, &mut in_word);
    Ok(words)
}

fn flush_word(words: &mut Vec<String>, cur: &mut String, in_word: &mut bool) {
    if *in_word {
        words.push(std::mem::take(cur));
        *in_word = false;
    }
}

fn window_cmd(name: &str, args: &[String]) -> Option<Result<Command, String>> {
    let cmd = match name {
        "split-window" | "split" | "sp" => split(name, args, false),
        "vsplit" | "vs" => split(name, args, true),
        "new-window" | "new" | "tabnew" => new_window(name, args),
        "kill-window" => kill_window(name, args),
        "rename-window" => rename_window(args),
        "select-window" => select_window(name, args),
        "next-window" if args == ["-a"] => {
            Ok(Command::Window(WindowCommand::StepAlert { forward: true }))
        }
        "previous-window" if args == ["-a"] => {
            Ok(Command::Window(WindowCommand::StepAlert { forward: false }))
        }
        "next-window" | "tabnext" | "tabn" => {
            bare(name, args, Command::Window(WindowCommand::NextWindow))
        }
        "previous-window" | "prev-window" | "tabprevious" | "tabp" => {
            bare(name, args, Command::Window(WindowCommand::PrevWindow))
        }
        "last-window" => bare(name, args, Command::Window(WindowCommand::LastWindow)),
        _ => return None,
    };
    Some(cmd)
}

fn pane_cmd(name: &str, args: &[String]) -> Option<Result<Command, String>> {
    let cmd = match name {
        "kill-pane" => kill_pane(name, args),
        "mark-pane" => mark_pane(name, args),
        "select-pane" => select_pane(name, args),
        "send-keys" => send_keys(name, args),
        "paste" => paste(name, args),
        "resize-pane" => resize_pane(name, args),
        "swap-pane" => swap_pane(name, args),
        "join-pane" | "joinp" => join_pane(name, args),
        "respawn-pane" | "respawnp" => respawn_pane(name, args),
        "find-window" | "findw" => find_window(name, args),
        "break-pane" => bare(name, args, Command::Pane(PaneCommand::BreakPane)),
        "last-pane" => last_pane(name, args),
        "next-pane" => bare(name, args, Command::Pane(PaneCommand::NextPane)),
        "select-layout" => select_layout(name, args),
        "next-layout" => bare(
            name,
            args,
            Command::Pane(PaneCommand::SelectLayout { preset: None }),
        ),
        _ => return None,
    };
    Some(cmd)
}

fn misc_cmd(
    name: &str,
    args: &[String],
    aliases: &BTreeMap<String, String>,
) -> Option<Result<Command, String>> {
    let cmd = match name {
        "copy-mode" => copy_mode(name, args),
        "detach-client" | "detach" | "q" | "quit" => bare(name, args, Command::Detach),
        "kill-server" | "qa" => bare(
            name,
            args,
            Command::Confirm {
                prompt: "kill-server? (y/n)".to_string(),
                then: Box::new(Command::KillServer),
            },
        ),
        "rotate-window" | "rotatew" => rotate_window(name, args),
        "move-window" | "movew" => move_window(name, args),
        "swap-window" | "swapw" => swap_window(name, args),
        "display-message" | "display" => display_message(name, args),
        "choose-tree" => choose_tree(name, args),
        "choose-buffer" => bare(name, args, Command::ChooseBuffer),
        "display-panes" | "displayp" => display_panes(name, args),
        "clock-mode" => clock_mode(name, args),
        "show-messages" | "showmsgs" => bare(name, args, Command::ShowMessages),
        "list-buffers" | "lsb" => list_buffers(name, args),
        "set-buffer" | "setb" => set_buffer(name, args),
        "show-buffer" | "showb" => {
            named_buffer(name, args, |name| BufferCommand::ShowBuffer { name })
        }
        "delete-buffer" | "deleteb" => {
            named_buffer(name, args, |name| BufferCommand::DeleteBuffer { name })
        }
        "load-buffer" | "loadb" => load_buffer(name, args),
        "save-buffer" | "saveb" => save_buffer(name, args),
        "paste-buffer" | "pasteb" => paste_buffer(name, args),
        "capture-pane" | "capturep" => capture_pane(name, args),
        "set-environment" | "setenv" => set_environment(name, args),
        "show-environment" | "showenv" => show_environment(name, args),
        "suspend-client" | "suspendc" => bare(name, args, Command::SuspendClient),
        "set-option" | "set" | "set-window-option" | "setw" => set_option(name, args),
        "show-options" | "show" | "show-window-options" | "showw" => show_options(name, args),
        "list-keys" | "help" => bare(name, args, Command::OpenHelp),
        "send-prefix" => bare(name, args, Command::SendPrefix),
        "reload-config" => bare(name, args, Command::ReloadConfig),
        "source-file" if args.is_empty() => Ok(Command::ReloadConfig),
        "source-file" => source_file(name, args).map(Command::Script),
        "run-shell" => run_shell(name, args).map(Command::Script),
        "if-shell" => if_shell(name, args, aliases).map(Command::Script),
        "wait-for" => wait_for(name, args).map(Command::Script),
        "set-hook" => set_hook(name, args, aliases).map(Command::Script),
        "show-hooks" => bare(name, args, Command::Script(ScriptCommand::ShowHooks)),
        "bind-key" | "bind" => bind_key(name, args),
        "unbind-key" | "unbind" => unbind_key(name, args),
        "next-session" => bare(name, args, Command::Session(SessionCommand::NextSession)),
        "prev-session" => bare(name, args, Command::Session(SessionCommand::PrevSession)),
        "switch-client" | "session" => switch_client(name, args),
        "rename-session" | "rename" => rename_session(name, args),
        "palette" | "command-palette" => palette_cmd(name, args),
        "command-prompt" => command_prompt(name, args),
        "confirm-before" | "confirm" => confirm_before(name, args),
        _ => return None,
    };
    Some(cmd)
}

fn bare(name: &str, args: &[String], cmd: Command) -> Result<Command, String> {
    if args.is_empty() {
        Ok(cmd)
    } else {
        Err(format!("{name}: takes no arguments"))
    }
}

const ROTATE: Spec = Spec {
    bools: &["-D", "-U"],
    values: &[],
    stop_at_arg: false,
};
const DISPLAY_MESSAGE: Spec = Spec {
    bools: &["-p"],
    values: &[],
    stop_at_arg: true,
};

fn rotate_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, ROTATE)?;
    o.no_rest(name)?;
    Ok(Command::Window(WindowCommand::RotateWindow { down: o.has("-D") }))
}

fn move_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, MOVE_WINDOW)?;
    o.no_rest(name)?;
    if o.has("-r") {
        return Ok(Command::Window(WindowCommand::RenumberWindows));
    }
    let raw = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <index> or -r"))?;
    let to = parse_index(raw).map_err(|_| format!("{name}: invalid window index '{raw}'"))?;
    Ok(Command::Window(WindowCommand::MoveWindow { to }))
}

const JOIN_PANE: Spec = Spec {
    bools: &["-h", "-v"],
    values: &["-s", "-t"],
    stop_at_arg: false,
};
const SWAP_WINDOW: Spec = Spec {
    bools: &[],
    values: &["-s", "-t"],
    stop_at_arg: false,
};
const RESPAWN_PANE: Spec = Spec {
    bools: &["-k"],
    values: &["-c", "-t"],
    stop_at_arg: true,
};

fn join_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, JOIN_PANE)?;
    o.no_rest(name)?;
    Ok(Command::Pane(PaneCommand::JoinPane {
        src: o.value("-s").map(str::to_string),
        dst: o.value("-t").map(str::to_string),
        side_by_side: o.has("-h"),
    }))
}

fn window_index(name: &str, raw: &str) -> Result<usize, String> {
    parse_index(raw).map_err(|_| format!("{name}: invalid window index '{raw}'"))
}

fn swap_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SWAP_WINDOW)?;
    o.no_rest(name)?;
    let dst = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <index>"))?;
    let src = o.value("-s").map(|s| window_index(name, s)).transpose()?;
    Ok(Command::Window(WindowCommand::SwapWindow {
        src,
        dst: window_index(name, dst)?,
    }))
}

fn respawn_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, RESPAWN_PANE)?;
    Ok(Command::Pane(PaneCommand::RespawnPane {
        target: o.value("-t").map(str::to_string),
        kill: o.has("-k"),
        cwd: o.value("-c").map(str::to_string),
        command: o.rest,
    }))
}

const FIND_WINDOW: Spec = Spec {
    bools: &["-i", "-N", "-T"],
    values: &[],
    stop_at_arg: false,
};

fn find_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = with_operands(name, args, FIND_WINDOW, "need a pattern")?;
    Ok(Command::Window(WindowCommand::FindWindow {
        pattern: o.rest.join(" "),
    }))
}

/// The options of `name`, which must be followed by operands; `missing` says what they are.
fn with_operands(name: &str, args: &[String], spec: Spec, missing: &str) -> Result<Opts, String> {
    let o = parse_opts(name, args, spec)?;
    if o.rest.is_empty() {
        return Err(format!("{name}: {missing}"));
    }
    Ok(o)
}

fn display_message(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, DISPLAY_MESSAGE)?;
    let format = (!o.rest.is_empty()).then(|| o.rest.join(" "));
    Ok(Command::DisplayMessage { format })
}

const SET_OPTION: Spec = Spec {
    bools: &["-g", "-w", "-s", "-p", "-q", "-u"],
    values: &[],
    stop_at_arg: false,
};
const SHOW_OPTIONS: Spec = Spec {
    bools: &["-g", "-w", "-s", "-q", "-v"],
    values: &[],
    stop_at_arg: false,
};

fn set_option(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SET_OPTION)?;
    if o.has("-p") {
        return Err(format!("{name}: mpx has no pane options"));
    }
    let unset = o.has("-u");
    if o.has("-s") && o.rest.first().is_some_and(|option| is_command_alias(option)) {
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
        _ => Err(format!("{name}: command-alias needs name=command, or -u name")),
    }
}

const RUN_SHELL: Spec = Spec {
    bools: &["-b"],
    values: &["-t", "-d"],
    stop_at_arg: true,
};
const IF_SHELL: Spec = Spec {
    bools: &["-b", "-F"],
    values: &["-t"],
    stop_at_arg: true,
};
const WAIT_FOR: Spec = Spec {
    bools: &["-L", "-S", "-U"],
    values: &[],
    stop_at_arg: false,
};
const SET_HOOK: Spec = Spec {
    bools: &["-a", "-g", "-R", "-u", "-w"],
    values: &["-t"],
    stop_at_arg: true,
};
const SOURCE_FILE: Spec = Spec {
    bools: &["-q"],
    values: &[],
    stop_at_arg: false,
};

/// `run-shell [-b] [-t pane] [-d seconds] command`: the words after the flags form the command.
fn run_shell(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    let o = parse_opts(name, args, RUN_SHELL)?;
    if o.rest.is_empty() {
        return Err(format!("{name}: need a shell command"));
    }
    let delay_ms = o.value("-d").map(|raw| parse_delay(name, raw)).transpose()?;
    Ok(ScriptCommand::RunShell {
        background: o.has("-b"),
        target: o.value("-t").map(str::to_string),
        delay_ms,
        command: o.rest.join(" "),
    })
}

/// A delay in seconds as milliseconds; fractions are allowed.
fn parse_delay(name: &str, raw: &str) -> Result<u64, String> {
    match raw.parse::<f64>() {
        Ok(secs) if secs.is_finite() && secs >= 0.0 => Ok((secs * 1000.0).round() as u64),
        _ => Err(format!("{name}: invalid delay '{raw}'")),
    }
}

/// `if-shell [-bF] [-t pane] shell-command command [command]`.
fn if_shell(
    name: &str,
    args: &[String],
    aliases: &BTreeMap<String, String>,
) -> Result<ScriptCommand, String> {
    let o = parse_opts(name, args, IF_SHELL)?;
    let (condition, branches) = match o.rest.as_slice() {
        [condition, branches @ ..] if matches!(branches.len(), 1 | 2) => (condition, branches),
        _ => return Err(format!("{name}: need a condition and one or two commands")),
    };
    let then = parse_in(&branches[0], aliases)?;
    let otherwise = branches
        .get(1)
        .map(|line| parse_in(line, aliases).map(Box::new))
        .transpose()?;
    Ok(ScriptCommand::IfShell {
        background: o.has("-b"),
        format: o.has("-F"),
        target: o.value("-t").map(str::to_string),
        condition: condition.clone(),
        then: Box::new(then),
        otherwise,
    })
}

/// `wait-for [-L|-S|-U] channel`: `-S` signals, `-L` locks, `-U` unlocks; with none it waits.
fn wait_for(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    let o = parse_opts(name, args, WAIT_FOR)?;
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
}

/// `set-hook [-a] hook command`, `set-hook -u hook`, `set-hook -R hook`. Window and pane hooks
/// (`-w`, `-t`) are refused: a hook applies to the whole session.
fn set_hook(
    name: &str,
    args: &[String],
    aliases: &BTreeMap<String, String>,
) -> Result<ScriptCommand, String> {
    let o = parse_opts(name, args, SET_HOOK)?;
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
    parse_in(&line, aliases)?;
    Ok(ScriptCommand::SetHook {
        name: hook.clone(),
        command: Some(line),
        append: o.has("-a"),
    })
}

/// `source-file [-q] path...`: the commands of each file, run in turn.
fn source_file(name: &str, args: &[String]) -> Result<ScriptCommand, String> {
    let o = with_operands(name, args, SOURCE_FILE, "need a file")?;
    Ok(ScriptCommand::SourceFile {
        paths: o.rest.clone(),
        quiet: o.has("-q"),
    })
}

fn show_options(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SHOW_OPTIONS)?;
    if o.rest.len() > 1 {
        return Err(format!("{name}: too many arguments"));
    }
    Ok(Command::ShowOptions {
        name: o.rest.first().cloned(),
        value_only: o.has("-v"),
        quiet: o.has("-q"),
    })
}

fn last_pane(name: &str, args: &[String]) -> Result<Command, String> {
    bare(name, args, Command::Pane(PaneCommand::LastPane))
}

/// Option shape for one command: which flags are switches, which take a value, and whether
/// option parsing stops at the first positional word (for trailing command lines and keys).
#[derive(Clone, Copy)]
struct Spec {
    bools: &'static [&'static str],
    values: &'static [&'static str],
    stop_at_arg: bool,
}

const SPLIT: Spec = Spec {
    bools: &["-h", "-d"],
    values: &["-c"],
    stop_at_arg: true,
};
const NEW_WINDOW: Spec = Spec {
    bools: &[],
    values: &["-n", "-c"],
    stop_at_arg: true,
};
const KILL_TARGET: Spec = Spec {
    bools: &[],
    values: &["-t"],
    stop_at_arg: false,
};

const MOVE_WINDOW: Spec = Spec {
    bools: &["-r"],
    values: &["-t"],
    stop_at_arg: false,
};
const SELECT_PANE: Spec = Spec {
    bools: &["-L", "-R", "-U", "-D", "-m", "-M", "-Z"],
    values: &["-t"],
    stop_at_arg: false,
};
const RESIZE_PANE: Spec = Spec {
    bools: &["-L", "-R", "-U", "-D", "-Z"],
    values: &["-t"],
    stop_at_arg: false,
};
const SWAP_PANE: Spec = Spec {
    bools: &["-U", "-D", "-Z"],
    values: &["-s", "-t"],
    stop_at_arg: false,
};
const SEND_KEYS: Spec = Spec {
    bools: &["-X"],
    values: &["-t", "-N"],
    stop_at_arg: true,
};
const PASTE: Spec = Spec {
    bools: &["--submit"],
    values: &["-t"],
    stop_at_arg: true,
};
const SWITCH: Spec = Spec {
    bools: &[],
    values: &["-t"],
    stop_at_arg: false,
};
const RENAME: Spec = Spec {
    bools: &[],
    values: &["-t"],
    stop_at_arg: false,
};
const COMMAND_PROMPT: Spec = Spec {
    bools: &["-1", "-b", "-F", "-k", "-N"],
    values: &["-I", "-p", "-T", "-t"],
    stop_at_arg: true,
};
const CONFIRM: Spec = Spec {
    bools: &["-b", "-y"],
    values: &["-p", "-t"],
    stop_at_arg: true,
};
const DISPLAY_PANES: Spec = Spec {
    bools: &[],
    values: &["-d"],
    stop_at_arg: false,
};
const CLOCK_MODE: Spec = Spec {
    bools: &[],
    values: &["-t"],
    stop_at_arg: false,
};
const COPY_MODE: Spec = Spec {
    bools: &["-e", "-H", "-M", "-q", "-u"],
    values: &["-t"],
    stop_at_arg: false,
};
/// `choose-tree` flags: `-s` (sessions) and `-w` (windows) pick what the tree opens on, `-Z` zooms.
const CHOOSE_TREE: Spec = Spec {
    bools: &["-s", "-w", "-Z"],
    values: &[],
    stop_at_arg: false,
};

#[derive(Debug, Default)]
struct Opts {
    flags: Vec<String>,
    values: Vec<(String, String)>,
    rest: Vec<String>,
}

impl Opts {
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    fn value(&self, flag: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(f, _)| f == flag)
            .map(|(_, v)| v.as_str())
    }

    fn no_rest(&self, name: &str) -> Result<(), String> {
        match self.rest.first() {
            Some(extra) => Err(format!("{name}: unexpected argument '{extra}'")),
            None => Ok(()),
        }
    }
}

fn parse_opts(name: &str, args: &[String], spec: Spec) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if a == "--" {
            o.rest.extend_from_slice(&args[i..]);
            break;
        }
        if !is_flag(a) {
            o.rest.push(a.to_string());
            if spec.stop_at_arg {
                o.rest.extend_from_slice(&args[i..]);
                break;
            }
        } else if listed(spec.values, a) {
            let v = args
                .get(i)
                .ok_or_else(|| format!("{name}: {a} needs a value"))?;
            i += 1;
            o.values.push((a.to_string(), v.clone()));
        } else {
            take_switches(name, a, spec.bools, &mut o.flags)?;
        }
    }
    Ok(o)
}

fn is_flag(a: &str) -> bool {
    a.len() > 1 && a.starts_with('-')
}

fn listed(list: &[&str], flag: &str) -> bool {
    list.contains(&flag)
}

/// `--long` is one switch; `-abc` is three short switches.
fn take_switches(name: &str, a: &str, bools: &[&str], out: &mut Vec<String>) -> Result<(), String> {
    if a.starts_with("--") {
        return if listed(bools, a) {
            out.push(a.to_string());
            Ok(())
        } else {
            Err(format!("{name}: unknown option {a}"))
        };
    }
    for ch in a[1..].chars() {
        let flag = format!("-{ch}");
        if !listed(bools, &flag) {
            return Err(format!("{name}: unknown option {flag}"));
        }
        out.push(flag);
    }
    Ok(())
}

fn parse_index(s: &str) -> Result<usize, String> {
    s.strip_prefix(':')
        .unwrap_or(s)
        .parse::<usize>()
        .map_err(|_| format!("invalid window index '{s}'"))
}

fn direction_flag(o: &Opts) -> Option<Side> {
    [
        ("-L", Side::Left),
        ("-R", Side::Right),
        ("-U", Side::Up),
        ("-D", Side::Down),
    ]
    .into_iter()
    .find(|(flag, _)| o.has(flag))
    .map(|(_, side)| side)
}

fn split(name: &str, args: &[String], vertical_alias: bool) -> Result<Command, String> {
    let o = parse_opts(name, args, SPLIT)?;
    let side_by_side = vertical_alias || o.has("-h");
    let cwd = o.value("-c").map(str::to_string);
    let detach = o.has("-d");
    Ok(Command::Pane(PaneCommand::SplitWindow {
        side_by_side,
        argv: o.rest,
        cwd,
        detach,
    }))
}

fn new_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, NEW_WINDOW)?;
    let window = o.value("-n").map(str::to_string);
    let cwd = o.value("-c").map(str::to_string);
    Ok(Command::Window(WindowCommand::NewWindow {
        name: window,
        argv: o.rest,
        cwd,
    }))
}

fn kill_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    let index = o.value("-t").map(parse_index).transpose()?;
    Ok(Command::Window(WindowCommand::KillWindow { index }))
}

fn rename_window(args: &[String]) -> Result<Command, String> {
    // tmux's `rename-window -- name` ends the options: the name may start with a dash.
    let words = args.strip_prefix(&["--".to_string()]).unwrap_or(args);
    let name = words.join(" ");
    if name.trim().is_empty() {
        return Err("rename-window: missing name".to_string());
    }
    Ok(Command::Window(WindowCommand::RenameWindow { name }))
}

fn select_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    let raw = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <index>"))?;
    match parse_index(raw) {
        Ok(index) => Ok(Command::Window(WindowCommand::SelectWindow { index })),
        Err(_) => Ok(Command::Window(WindowCommand::SelectWindowNamed {
            name: raw.rsplit(':').next().unwrap_or(raw).to_string(),
        })),
    }
}

fn kill_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    Ok(Command::Pane(PaneCommand::KillPane {
        target: o.value("-t").map(str::to_string),
    }))
}

fn select_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SELECT_PANE)?;
    o.no_rest(name)?;
    if o.has("-Z") {
        return Err(format!("{name}: -Z is not supported"));
    }
    if o.has("-M") {
        return Ok(Command::Pane(PaneCommand::ClearMark));
    }
    if o.has("-m") {
        return Ok(Command::Pane(PaneCommand::MarkPane {
            target: o.value("-t").map(str::to_string),
        }));
    }
    if let Some(side) = direction_flag(&o) {
        return Ok(Command::Pane(PaneCommand::SelectPaneDir { side }));
    }
    let target = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <target>, -m/-M, or -L/-R/-U/-D"))?;
    Ok(Command::Pane(PaneCommand::SelectPane {
        target: target.to_string(),
    }))
}

fn mark_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SELECT_PANE)?;
    o.no_rest(name)?;
    if o.has("-M") {
        Ok(Command::Pane(PaneCommand::ClearMark))
    } else {
        Ok(Command::Pane(PaneCommand::MarkPane {
            target: o.value("-t").map(str::to_string),
        }))
    }
}

fn resize_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, RESIZE_PANE)?;
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
}

fn swap_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SWAP_PANE)?;
    o.no_rest(name)?;
    if o.has("-Z") {
        return Err(format!("{name}: -Z is not supported"));
    }
    if o.has("-D") {
        Ok(Command::Pane(PaneCommand::SwapPane { next: true }))
    } else if o.has("-U") {
        Ok(Command::Pane(PaneCommand::SwapPane { next: false }))
    } else {
        Ok(Command::Pane(PaneCommand::SwapPaneMarked {
            source: o.value("-s").map(str::to_string),
            target: o.value("-t").map(str::to_string),
        }))
    }
}

fn select_layout(name: &str, args: &[String]) -> Result<Command, String> {
    let preset = match args {
        [flag] if flag == "-E" => return Ok(Command::Pane(PaneCommand::SpreadLayout)),
        [] => None,
        [one] => Some(LayoutPreset::parse(one).ok_or_else(|| format!("unknown layout '{one}'"))?),
        _ => return Err(format!("{name}: too many arguments")),
    };
    Ok(Command::Pane(PaneCommand::SelectLayout { preset }))
}

fn send_keys(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SEND_KEYS)?;
    let target = o.value("-t").map(str::to_string);
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
        let action = super::copy::parse_copy_command(command, command_args)?;
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
}

fn paste(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, PASTE)?;
    Ok(Command::Paste {
        target: o.value("-t").map(str::to_string),
        text: o.rest.join(" "),
        submit: o.has("--submit"),
    })
}

fn target_or_first(
    name: &str,
    args: &[String],
    spec: Spec,
    missing: &str,
) -> Result<String, String> {
    let o = parse_opts(name, args, spec)?;
    o.value("-t")
        .map(str::to_string)
        .or_else(|| o.rest.first().cloned())
        .ok_or_else(|| format!("{name}: {missing}"))
}

fn switch_client(name: &str, args: &[String]) -> Result<Command, String> {
    let session = target_or_first(name, args, SWITCH, "need -t <session>")?;
    Ok(Command::Session(SessionCommand::SwitchSession { name: session }))
}

fn rename_session(name: &str, args: &[String]) -> Result<Command, String> {
    let session = target_or_first(name, args, RENAME, "need new name")?;
    Ok(Command::Session(SessionCommand::RenameSession { name: session }))
}
fn command_prompt(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, COMMAND_PROMPT)?;
    let kind = match o.value("-T") {
        None | Some("command") => PromptKind::Command,
        Some("search") => PromptKind::Search,
        Some("target") => PromptKind::Target,
        Some("window-target") => PromptKind::WindowTarget,
        Some(other) => return Err(format!("{name}: unknown prompt type '{other}'")),
    };
    let template = (!o.rest.is_empty()).then(|| o.rest.join(" "));
    Ok(Command::CommandPrompt(PromptSpec {
        prompts: split_list(o.value("-p")),
        initial: split_list(o.value("-I")),
        template,
        kind,
        single_key: o.has("-1") || o.has("-k"),
        numeric: o.has("-N"),
        expand: o.has("-F"),
    }))
}

/// A comma-separated option value as its parts; an absent option has none.
fn split_list(value: Option<&str>) -> Vec<String> {
    value.map_or_else(Vec::new, |v| v.split(',').map(str::to_string).collect())
}

/// `confirm-before [-y] [-p prompt] command`: runs `command` once the user answers `y`; `-y`
/// runs it at once.
fn confirm_before(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, CONFIRM)?;
    if o.rest.is_empty() {
        return Err(format!("{name}: need a command"));
    }
    let line = crate::cmdline::command_line(&o.rest);
    let then = parse(&line)?;
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
}

fn display_panes(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, DISPLAY_PANES)?;
    o.no_rest(name)?;
    let duration_ms = match o.value("-d") {
        Some(ms) => Some(
            ms.parse::<u64>()
                .map_err(|_| format!("{name}: bad duration '{ms}'"))?,
        ),
        None => None,
    };
    Ok(Command::DisplayPanes { duration_ms })
}

fn clock_mode(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, CLOCK_MODE)?;
    o.no_rest(name)?;
    Ok(Command::ClockMode {
        target: o.value("-t").map(str::to_string),
    })
}

fn palette_cmd(name: &str, args: &[String]) -> Result<Command, String> {
    let tab = match args.first().map(|s| s.as_str()) {
        None => None,
        Some("commands" | "cmd") => Some(PaletteTab::Commands),
        Some("sessions" | "session" | "tree") => Some(PaletteTab::Sessions),
        Some("help" | "keys") => Some(PaletteTab::Help),
        Some(other) => return Err(format!("{name}: unknown tab '{other}'")),
    };
    Ok(Command::OpenPalette { tab })
}

/// `copy-mode [-eHMqu] [-t pane]`. `-H` (hide position) and `-M` (mouse) are accepted; mpx has no
/// position indicator to hide and mouse drags always start copy mode.
fn copy_mode(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, COPY_MODE)?;
    o.no_rest(name)?;
    Ok(Command::CopyMode(CopyModeOpts {
        target: o.value("-t").map(str::to_string),
        page_up: o.has("-u"),
        exit_at_bottom: o.has("-e"),
        quit: o.has("-q"),
    }))
}

/// `choose-tree [-swZ]`: the tree overlay; the flags are accepted, the overlay opens the same way.
fn choose_tree(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, CHOOSE_TREE)?;
    o.no_rest(name)?;
    Ok(Command::OpenTree)
}

const SET_BUFFER: Spec = Spec {
    bools: &["-a"],
    values: &["-b", "-n"],
    stop_at_arg: true,
};
const BUFFER_OF: Spec = Spec {
    bools: &[],
    values: &["-b"],
    stop_at_arg: false,
};
const SAVE_BUFFER: Spec = Spec {
    bools: &["-a"],
    values: &["-b"],
    stop_at_arg: false,
};
const PASTE_BUFFER: Spec = Spec {
    bools: &["-d", "-p", "-r"],
    values: &["-b", "-s", "-t"],
    stop_at_arg: false,
};
const LIST_BUFFERS: Spec = Spec {
    bools: &[],
    values: &["-F"],
    stop_at_arg: false,
};
const CAPTURE_PANE: Spec = Spec {
    bools: &["-p", "-e", "-J"],
    values: &["-t", "-S", "-E", "-b"],
    stop_at_arg: false,
};
const SET_ENVIRONMENT: Spec = Spec {
    bools: &["-g", "-u", "-r"],
    values: &["-t"],
    stop_at_arg: true,
};
const SHOW_ENVIRONMENT: Spec = Spec {
    bools: &["-g"],
    values: &["-t"],
    stop_at_arg: false,
};

/// Whether a buffer command takes operands after its options.
enum Operands {
    /// Any word left after the options is an error, reported before the command is built.
    None,
    /// The builder reads the operands itself (`o.rest`).
    Any,
}

/// Parses the options of a paste-buffer command with `spec`, then builds the command from them.
fn buffer_cmd(
    name: &str,
    args: &[String],
    spec: Spec,
    operands: Operands,
    build: impl FnOnce(&str, &Opts) -> Result<BufferCommand, String>,
) -> Result<Command, String> {
    let o = parse_opts(name, args, spec)?;
    if matches!(operands, Operands::None) {
        o.no_rest(name)?;
    }
    build(name, &o).map(Command::Buffer)
}

/// Parses the options of an environment command with `spec`, then builds the command from them.
fn env_cmd(
    name: &str,
    args: &[String],
    spec: Spec,
    build: impl FnOnce(&str, &Opts) -> Result<EnvCommand, String>,
) -> Result<Command, String> {
    let o = parse_opts(name, args, spec)?;
    build(name, &o).map(Command::Environment)
}

fn list_buffers(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, LIST_BUFFERS, Operands::None, |_, o| {
        Ok(BufferCommand::ListBuffers {
            format: o.value("-F").map(str::to_string),
        })
    })
}

fn set_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, SET_BUFFER, Operands::Any, |name, o| {
        let rename = o.value("-n").map(str::to_string);
        let data = (!o.rest.is_empty()).then(|| o.rest.join(" "));
        match (rename.is_some(), data.is_some()) {
            (true, true) => Err(format!("{name}: -n takes no data")),
            (false, false) => Err(format!("{name}: no data given")),
            _ => Ok(BufferCommand::SetBuffer {
                name: o.value("-b").map(str::to_string),
                append: o.has("-a"),
                rename,
                data,
            }),
        }
    })
}

/// `show-buffer` and `delete-buffer`: an optional `-b name` and no operands.
fn named_buffer(
    name: &str,
    args: &[String],
    make: fn(Option<String>) -> BufferCommand,
) -> Result<Command, String> {
    buffer_cmd(name, args, BUFFER_OF, Operands::None, |_, o| {
        Ok(make(o.value("-b").map(str::to_string)))
    })
}

/// The one path operand of `load-buffer` and `save-buffer`.
fn single_path(name: &str, o: &Opts) -> Result<String, String> {
    match o.rest.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!("{name}: need a path")),
        _ => Err(format!("{name}: too many arguments")),
    }
}

fn load_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, BUFFER_OF, Operands::Any, |name, o| {
        let path = single_path(name, o)?;
        Ok(BufferCommand::LoadBuffer {
            name: o.value("-b").map(str::to_string),
            path,
        })
    })
}

fn save_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, SAVE_BUFFER, Operands::Any, |name, o| {
        let path = single_path(name, o)?;
        Ok(BufferCommand::SaveBuffer {
            name: o.value("-b").map(str::to_string),
            append: o.has("-a"),
            path,
        })
    })
}

fn paste_buffer(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, PASTE_BUFFER, Operands::None, |_, o| {
        Ok(BufferCommand::PasteBuffer {
            opts: PasteBufferOpts {
                name: o.value("-b").map(str::to_string),
                pane: o.value("-t").unwrap_or_default().to_string(),
                delete: o.has("-d"),
                bracketed: o.has("-p"),
                raw: o.has("-r"),
                separator: o.value("-s").map(str::to_string),
            },
        })
    })
}

fn capture_pane(name: &str, args: &[String]) -> Result<Command, String> {
    buffer_cmd(name, args, CAPTURE_PANE, Operands::None, |_, o| {
        Ok(BufferCommand::CapturePane {
            target: o.value("-t").map(str::to_string),
            print: o.has("-p"),
            buffer: o.value("-b").map(str::to_string),
            spec: crate::cli::capture_spec(o.value("-S"), o.value("-E"), o.has("-e"), o.has("-J"))?,
        })
    })
}

fn set_environment(name: &str, args: &[String]) -> Result<Command, String> {
    env_cmd(name, args, SET_ENVIRONMENT, |name, o| {
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
}

fn show_environment(name: &str, args: &[String]) -> Result<Command, String> {
    env_cmd(name, args, SHOW_ENVIRONMENT, |name, o| {
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
}
