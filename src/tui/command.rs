//! Typed TUI commands. Key bindings, the `:` prompt and overlays all produce `Command`s.
//! Targets use `[session:]pane` where pane is a node name or `%id`.
use crate::protocol::{LayoutPreset, Side};

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
    OpenAgents,
    /// Jump to the next agent (any session) in `needs_input`.
    NextAttention,
    OpenTree,
    OpenHelp,
    /// Focus the agents rail, expanding it, or collapse it again.
    ToggleSidebar,
    /// Show the next (or previous) agent in the rail without focusing it.
    StepAgent {
        next: bool,
    },
    CommandPrompt {
        initial: String,
    },
    LaunchAgent {
        harness: String,
        resume: Option<String>,
        cwd: Option<String>,
        split: bool,
    },
    /// Ask `prompt (y/n)` on the command line; run `then` on `y`.
    Confirm {
        prompt: String,
        then: Box<Command>,
    },
}

/// Command names and aliases accepted by `parse`; used for `:` completion.
pub const COMMAND_NAMES: &[&str] = &[
    "agents",
    "break-pane",
    "choose-tree",
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
    "launch-agent",
    "list-keys",
    "new",
    "new-window",
    "next-agent",
    "next-attention",
    "next-layout",
    "next-pane",
    "next-session",
    "next-window",
    "paste",
    "prev-agent",
    "prev-session",
    "prev-window",
    "previous-window",
    "q",
    "qa",
    "quit",
    "rename-window",
    "resize-pane",
    "select-layout",
    "select-pane",
    "select-window",
    "send-keys",
    "send-prefix",
    "session",
    "sidebar",
    "sp",
    "split",
    "split-window",
    "swap-pane",
    "switch-client",
    "tabn",
    "tabnext",
    "tabnew",
    "tabp",
    "tabprevious",
    "vs",
    "vsplit",
];

/// A family of command names; returns `None` when the name belongs to another family.
type Group = fn(&str, &[String]) -> Option<Result<Command, String>>;

/// Parses a command line (tmux command names plus vim aliases such as `vsplit`, `q`).
pub fn parse(line: &str) -> Result<Command, String> {
    let words = split_words(line)?;
    let (name, args) = words
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    let groups: [Group; 3] = [window_cmd, pane_cmd, misc_cmd];
    groups
        .into_iter()
        .find_map(|group| group(name, args))
        .unwrap_or_else(|| Err(format!("unknown command: {name}")))
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
        "agents" => bare(name, args, Command::OpenAgents),
        "next-attention" => bare(name, args, Command::NextAttention),
        "choose-tree" => bare(name, args, Command::OpenTree),
        "list-keys" | "help" => bare(name, args, Command::OpenHelp),
        "sidebar" => bare(name, args, Command::ToggleSidebar),
        "next-agent" => bare(name, args, Command::StepAgent { next: true }),
        "prev-agent" => bare(name, args, Command::StepAgent { next: false }),
        "send-prefix" => bare(name, args, Command::SendPrefix),
        "next-session" => bare(name, args, Command::NextSession),
        "prev-session" => bare(name, args, Command::PrevSession),
        "switch-client" | "session" => switch_client(name, args),
        "launch-agent" => launch_agent(name, args),
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
    bools: &["-L", "-R", "-U", "-D"],
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
    values: &[],
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
const LAUNCH: Spec = Spec {
    bools: &["--split"],
    values: &["--resume", "--cwd"],
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
    if let Some(side) = direction_flag(&o) {
        return Ok(Command::SelectPaneDir { side });
    }
    let target = o
        .value("-t")
        .ok_or_else(|| format!("{name}: need -t <target> or -L/-R/-U/-D"))?;
    Ok(Command::SelectPane {
        target: target.to_string(),
    })
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
        Err(format!("{name}: need -U or -D"))
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
    if o.rest.is_empty() {
        return Err(format!("{name}: missing text"));
    }
    Ok(Command::Paste {
        target: o.value("-t").map(str::to_string),
        text: o.rest.join(" "),
        submit: o.has("--submit"),
    })
}

fn switch_client(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, SWITCH)?;
    let session = match o.value("-t") {
        Some(s) => s.to_string(),
        None => o
            .rest
            .first()
            .cloned()
            .ok_or_else(|| format!("{name}: need -t <session>"))?,
    };
    Ok(Command::SwitchSession { name: session })
}

fn launch_agent(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, LAUNCH)?;
    let harness = match o.rest.as_slice() {
        [one] => one.clone(),
        [] => return Err(format!("{name}: missing harness")),
        [_, extra, ..] => return Err(format!("{name}: unexpected argument '{extra}'")),
    };
    Ok(Command::LaunchAgent {
        harness,
        resume: o.value("--resume").map(str::to_string),
        cwd: o.value("--cwd").map(str::to_string),
        split: o.has("--split"),
    })
}

fn command_prompt(name: &str, args: &[String]) -> Result<Command, String> {
    let o = parse_opts(name, args, PROMPT)?;
    let initial = match o.value("-I") {
        Some(text) => text.to_string(),
        None => o.rest.join(" "),
    };
    Ok(Command::CommandPrompt { initial })
}
