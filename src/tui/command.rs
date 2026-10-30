//! Typed TUI commands. Key bindings, the `:` prompt and overlays all produce `Command`s.
//! Targets use `[session:]pane` where pane is a node name or `%id`.
use super::copy::CopyAction;
use super::keymap::Table;
use super::prompt::PromptKind;
use crate::protocol::{LayoutPreset, Side};

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
    /// `side_by_side`: vertical divider (`:vsplit`, tmux `split-window -h`).
    SplitWindow {
        side_by_side: bool,
        argv: Vec<String>,
        cwd: Option<String>,
        detach: bool,
    },
    NewWindow {
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
    },
    KillPane {
        target: Option<String>,
    },
    KillWindow {
        index: Option<usize>,
    },
    RenameWindow {
        name: String,
    },
    SelectPane {
        target: String,
    },
    SelectPaneDir {
        side: Side,
    },
    MarkPane {
        target: Option<String>,
    },
    ClearMark,
    SelectWindow {
        index: usize,
    },
    NextWindow,
    PrevWindow,
    LastWindow,
    NextPane,
    LastPane,
    SwitchSession {
        name: String,
    },
    NextSession,
    PrevSession,
    SendKeys {
        target: Option<String>,
        keys: Vec<String>,
    },
    Paste {
        target: Option<String>,
        text: String,
        submit: bool,
    },
    ResizePane {
        side: Side,
        cells: u16,
    },
    SwapPane {
        next: bool,
    },
    SwapPaneMarked {
        source: Option<String>,
        target: Option<String>,
    },
    BreakPane,
    SelectLayout {
        preset: Option<LayoutPreset>,
    },
    /// `select-layout -E`: spread the panes evenly, keeping their arrangement.
    SpreadLayout,
    Zoom,
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
    RenameSession {
        name: String,
    },
    OpenTree,
    OpenHelp,
    OpenPalette {
        tab: Option<PaletteTab>,
    },
    /// `command-prompt`: one question per `prompts` entry (`-p`), each starting with its `initial`
    /// text (`-I`). With a `template` the answers fill it in (`%%`, `%1`..`%9`); without one the
    /// typed line runs as a command.
    CommandPrompt(PromptSpec),
    Confirm {
        prompt: String,
        then: Box<Command>,
    },
    /// Commands run one after the other (`a ; b`), stopping at the first that fails or quits.
    Sequence(Vec<Command>),
    /// `bind-key`: bind `key` in `table`; an empty `line` is `unbind-key`.
    Bind {
        table: Table,
        key: String,
        line: String,
    },
    /// Re-read the session's options and key bindings (sent when another client changes them).
    RefreshRuntime,
    /// Any other tmux command, run as the `tmux` shim would against this session.
    Tmux {
        words: Vec<String>,
    },
    /// `display-panes [-d ms]`: number each visible pane until a digit picks one, a key is
    /// pressed or the time runs out. `None` takes `display-panes-time`; `Some(0)` waits for a key.
    DisplayPanes {
        duration_ms: Option<u64>,
    },
    /// `clock-mode [-t pane]`: a large clock fills the pane until a key is pressed.
    ClockMode {
        target: Option<String>,
    },
    /// `show-messages`: the status-row messages of this session, oldest first.
    ShowMessages,
    /// `list-buffers`: the paste buffers, one line each, in an overlay.
    ListBuffers,
    /// `choose-buffer`: the paste buffers, newest first; Enter pastes, `d` deletes.
    ChooseBuffer,
    /// `suspend-client`: stop the process as Ctrl-Z does; the TUI resumes on SIGCONT.
    SuspendClient,
}

/// Command names and aliases accepted by `parse`; used for `:` completion.
pub const COMMAND_NAMES: &[&str] = &[
    "bind-key",
    "break-pane",
    "choose-buffer",
    "choose-tree",
    "clock-mode",
    "command-palette",
    "command-prompt",
    "confirm-before",
    "copy-mode",
    "detach",
    "detach-client",
    "display-panes",
    "help",
    "kill-pane",
    "kill-server",
    "kill-window",
    "last-pane",
    "last-window",
    "list-buffers",
    "list-keys",
    "mark-pane",
    "new",
    "new-window",
    "next-layout",
    "next-pane",
    "next-session",
    "next-window",
    "palette",
    "paste",
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
    "select-layout",
    "select-pane",
    "select-window",
    "send-keys",
    "send-prefix",
    "session",
    "show-messages",
    "sp",
    "split",
    "split-window",
    "suspend-client",
    "swap-pane",
    "switch-client",
    "tabn",
    "tabnext",
    "tabnew",
    "source-file",
    "tabp",
    "tabprevious",
    "unbind-key",
    "vs",
    "vsplit",
];

/// A family of command names; returns `None` when the name belongs to another family.
type Group = fn(&str, &[String]) -> Option<Result<Command, String>>;

/// Parses a command line (tmux command names plus vim aliases such as `vsplit`, `q`).
pub fn parse(line: &str) -> Result<Command, String> {
    if line.contains(';') {
        let mut commands = Vec::new();
        for words in crate::tmux_shim::split_commands(line)? {
            commands.push(parse_words(&words)?);
        }
        return match commands.len() {
            0 => Err("empty command".to_string()),
            1 => Ok(commands.remove(0)),
            _ => Ok(Command::Sequence(commands)),
        };
    }
    parse_words(&split_words(line)?)
}

fn parse_words(words: &[String]) -> Result<Command, String> {
    let (name, args) = words
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    let groups: [Group; 3] = [window_cmd, pane_cmd, misc_cmd];
    groups
        .into_iter()
        .find_map(|group| group(name, args))
        .unwrap_or_else(|| tmux_fallback(name, args))
}

/// A tmux command with no TUI form runs through the shim, so every command the `tmux` shim knows
/// also works at the `:` prompt, in key bindings and in sourced files.
fn tmux_fallback(name: &str, args: &[String]) -> Result<Command, String> {
    if crate::tmux_shim::spec(crate::tmux_shim::canonical(name)).is_none() {
        return Err(format!("unknown command: {name}"));
    }
    let mut words = vec![name.to_string()];
    words.extend_from_slice(args);
    if crate::tmux_shim::canonical(name) == "display-message" {
        add_display_defaults(&mut words)?;
    }
    Ok(Command::Tmux { words })
}

/// `display-message` shows its text on the status row, so it prints (`-p`), and with no format it
/// describes the current pane as tmux does.
fn add_display_defaults(words: &mut Vec<String>) -> Result<(), String> {
    let inv = crate::tmux_shim::parse_invocation(words)?;
    if !inv.has('p') {
        words.insert(1, "-p".to_string());
    }
    if inv.operands.is_empty() {
        words.push(
            "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index} - (#{pane_title})"
                .to_string(),
        );
    }
    Ok(())
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
        return Err(format!("{name}: {} takes only send-keys -X commands", table.name()));
    }
    Ok(Command::Bind {
        table,
        key: key.clone(),
        line: crate::tmux_shim::command_line(command),
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
        "next-window" if args == ["-a"] => tmux_fallback(name, args),
        "previous-window" if args == ["-a"] => tmux_fallback(name, args),
        "next-window" | "tabnext" | "tabn" => bare(name, args, Command::NextWindow),
        "previous-window" | "prev-window" | "tabprevious" | "tabp" => {
            bare(name, args, Command::PrevWindow)
        }
        "last-window" => bare(name, args, Command::LastWindow),
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
        "break-pane" => bare(name, args, Command::BreakPane),
        "last-pane" => last_pane(name, args),
        "next-pane" => bare(name, args, Command::NextPane),
        "select-layout" => select_layout(name, args),
        "next-layout" => bare(name, args, Command::SelectLayout { preset: None }),
        _ => return None,
    };
    Some(cmd)
}

fn misc_cmd(name: &str, args: &[String]) -> Option<Result<Command, String>> {
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
        "choose-tree" => choose_tree(name, args),
        "choose-buffer" => bare(name, args, Command::ChooseBuffer),
        "display-panes" | "displayp" => display_panes(name, args),
        "clock-mode" => clock_mode(name, args),
        "show-messages" | "showmsgs" => bare(name, args, Command::ShowMessages),
        "list-buffers" | "lsb" if args.is_empty() => Ok(Command::ListBuffers),
        "list-buffers" | "lsb" => tmux_fallback(name, args),
        "suspend-client" | "suspendc" => bare(name, args, Command::SuspendClient),
        "list-keys" | "help" => bare(name, args, Command::OpenHelp),
        "send-prefix" => bare(name, args, Command::SendPrefix),
        "reload-config" => bare(name, args, Command::ReloadConfig),
        "source-file" if args.is_empty() => Ok(Command::ReloadConfig),
        "refresh-runtime" => bare(name, args, Command::RefreshRuntime),
        "bind-key" | "bind" => bind_key(name, args),
        "unbind-key" | "unbind" => unbind_key(name, args),
        "next-session" => bare(name, args, Command::NextSession),
        "prev-session" => bare(name, args, Command::PrevSession),
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

/// `last-pane` alone is the TUI's; with options (`-Z`) the shim handles it.
fn last_pane(name: &str, args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        Ok(Command::LastPane)
    } else {
        tmux_fallback(name, args)
    }
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
    Ok(Command::SplitWindow {
        side_by_side,
        argv: o.rest,
        cwd,
        detach,
    })
}

fn new_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, NEW_WINDOW)?;
    let window = o.value("-n").map(str::to_string);
    let cwd = o.value("-c").map(str::to_string);
    Ok(Command::NewWindow {
        name: window,
        argv: o.rest,
        cwd,
    })
}

fn kill_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    let index = o.value("-t").map(parse_index).transpose()?;
    Ok(Command::KillWindow { index })
}

fn rename_window(args: &[String]) -> Result<Command, String> {
    // tmux's `rename-window -- name` ends the options: the name may start with a dash.
    let words = args.strip_prefix(&["--".to_string()]).unwrap_or(args);
    let name = words.join(" ");
    if name.trim().is_empty() {
        return Err("rename-window: missing name".to_string());
    }
    Ok(Command::RenameWindow { name })
}

fn select_window(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    let raw = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <index>"))?;
    match parse_index(raw) {
        Ok(index) => Ok(Command::SelectWindow { index }),
        // A window name (`-t ':name'`) is resolved by the shim.
        Err(_) => tmux_fallback(name, args),
    }
}

fn kill_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, KILL_TARGET)?;
    o.no_rest(name)?;
    Ok(Command::KillPane {
        target: o.value("-t").map(str::to_string),
    })
}

fn select_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SELECT_PANE)?;
    o.no_rest(name)?;
    if o.has("-Z") {
        return tmux_fallback(name, args);
    }
    if o.has("-M") {
        return Ok(Command::ClearMark);
    }
    if o.has("-m") {
        return Ok(Command::MarkPane {
            target: o.value("-t").map(str::to_string),
        });
    }
    if let Some(side) = direction_flag(&o) {
        return Ok(Command::SelectPaneDir { side });
    }
    let target = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <target>, -m/-M, or -L/-R/-U/-D"))?;
    Ok(Command::SelectPane {
        target: target.to_string(),
    })
}

fn mark_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SELECT_PANE)?;
    o.no_rest(name)?;
    if o.has("-M") {
        Ok(Command::ClearMark)
    } else {
        Ok(Command::MarkPane {
            target: o.value("-t").map(str::to_string),
        })
    }
}

fn resize_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, RESIZE_PANE)?;
    if o.has("-Z") {
        return if o.value("-t").is_some() {
            tmux_fallback(name, args)
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
    Ok(Command::ResizePane { side, cells })
}

fn swap_pane(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SWAP_PANE)?;
    o.no_rest(name)?;
    if o.has("-Z") {
        return tmux_fallback(name, args);
    }
    if o.has("-D") {
        Ok(Command::SwapPane { next: true })
    } else if o.has("-U") {
        Ok(Command::SwapPane { next: false })
    } else {
        Ok(Command::SwapPaneMarked {
            source: o.value("-s").map(str::to_string),
            target: o.value("-t").map(str::to_string),
        })
    }
}

fn select_layout(name: &str, args: &[String]) -> Result<Command, String> {
    let preset = match args {
        [flag] if flag == "-E" => return Ok(Command::SpreadLayout),
        [] => None,
        [one] => Some(LayoutPreset::parse(one).ok_or_else(|| format!("unknown layout '{one}'"))?),
        _ => return Err(format!("{name}: too many arguments")),
    };
    Ok(Command::SelectLayout { preset })
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
    Ok(Command::SwitchSession { name: session })
}

fn rename_session(name: &str, args: &[String]) -> Result<Command, String> {
    let session = target_or_first(name, args, RENAME, "need new name")?;
    Ok(Command::RenameSession { name: session })
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
    let line = crate::tmux_shim::command_line(&o.rest);
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
