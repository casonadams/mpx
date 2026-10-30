//! Typed TUI commands. Key bindings, the `:` prompt and overlays all produce `Command`s.
//! Targets use `[session:]pane` where pane is a node name or `%id`.
use crate::protocol::{LayoutPreset, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteTab {
    #[default]
    Commands,
    Sessions,
    Help,
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
    CopyMode,
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
    CommandPrompt {
        initial: String,
    },
    Confirm {
        prompt: String,
        then: Box<Command>,
    },
    /// Commands run one after the other (`a ; b`), stopping at the first that fails or quits.
    Sequence(Vec<Command>),
    /// `bind-key`: bind `key` in the prefix table, or the root table with `root`; an empty `line`
    /// is `unbind-key`.
    Bind {
        root: bool,
        key: String,
        line: String,
    },
    /// Re-read the session's options and key bindings (sent when another client changes them).
    RefreshRuntime,
    /// Any other tmux command, run as the `tmux` shim would against this session.
    Tmux {
        words: Vec<String>,
    },
}

/// Command names and aliases accepted by `parse`; used for `:` completion.
pub const COMMAND_NAMES: &[&str] = &[
    "bind-key",
    "break-pane",
    "choose-tree",
    "command-palette",
    "command-prompt",
    "copy-mode",
    "detach",
    "detach-client",
    "help",
    "kill-pane",
    "kill-server",
    "kill-window",
    "last-pane",
    "last-window",
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
    "sp",
    "split",
    "split-window",
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
    Ok(Command::Tmux { words })
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

/// Whether `-n` or `-T` names the root table; tables other than prefix and root are refused.
fn root_table(name: &str, o: &Opts) -> Result<bool, String> {
    match (o.value("-T"), o.has("-n")) {
        (Some("root"), _) | (None, true) => Ok(true),
        (Some("prefix"), _) | (None, false) => Ok(false),
        (Some(table), _) => Err(format!("{name}: key table '{table}' is not supported")),
    }
}

fn bind_key(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, BIND)?;
    let root = root_table(name, &o)?;
    let (key, command) = o
        .rest
        .split_first()
        .ok_or_else(|| format!("{name}: need a key and a command"))?;
    if command.is_empty() {
        return Err(format!("{name}: need a command for key {key}"));
    }
    Ok(Command::Bind {
        root,
        key: key.clone(),
        line: crate::tmux_shim::command_line(command),
    })
}

fn unbind_key(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, UNBIND)?;
    let root = root_table(name, &o)?;
    let key = o
        .rest
        .first()
        .ok_or_else(|| format!("{name}: need a key"))?;
    Ok(Command::Bind {
        root,
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
        "last-pane" => bare(name, args, Command::LastPane),
        "next-pane" => bare(name, args, Command::NextPane),
        "select-layout" => select_layout(name, args),
        "next-layout" => bare(name, args, Command::SelectLayout { preset: None }),
        _ => return None,
    };
    Some(cmd)
}

fn misc_cmd(name: &str, args: &[String]) -> Option<Result<Command, String>> {
    let cmd = match name {
        "copy-mode" => bare(name, args, Command::CopyMode),
        "detach-client" | "detach" | "q" | "quit" => bare(name, args, Command::Detach),
        "kill-server" | "qa" => bare(
            name,
            args,
            Command::Confirm {
                prompt: "kill-server? (y/n)".to_string(),
                then: Box::new(Command::KillServer),
            },
        ),
        "choose-tree" => bare(name, args, Command::OpenTree),
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
    bools: &["-L", "-R", "-U", "-D", "-m", "-M"],
    values: &["-t"],
    stop_at_arg: false,
};
const RESIZE_PANE: Spec = Spec {
    bools: &["-L", "-R", "-U", "-D", "-Z"],
    values: &[],
    stop_at_arg: false,
};
const SWAP_PANE: Spec = Spec {
    bools: &["-U", "-D"],
    values: &["-s", "-t"],
    stop_at_arg: false,
};
const SEND_KEYS: Spec = Spec {
    bools: &[],
    values: &["-t"],
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
const PROMPT: Spec = Spec {
    bools: &[],
    values: &["-I"],
    stop_at_arg: true,
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
    let name = args.join(" ");
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
    Ok(Command::SelectWindow {
        index: parse_index(raw)?,
    })
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
        return Ok(Command::Zoom);
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
    if o.rest.is_empty() {
        return Err(format!("{name}: no keys given"));
    }
    Ok(Command::SendKeys {
        target: o.value("-t").map(str::to_string),
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
    let o = parse_opts(name, args, PROMPT)?;
    let initial = match o.value("-I") {
        Some(text) => text.to_string(),
        None => o.rest.join(" "),
    };
    Ok(Command::CommandPrompt { initial })
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
