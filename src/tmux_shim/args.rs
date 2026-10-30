use std::collections::{BTreeMap, BTreeSet};

/// A parsed tmux command line: canonical command name, option flags and trailing operands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Invocation {
    pub command: String,
    pub switches: BTreeSet<char>,
    pub values: BTreeMap<char, String>,
    pub operands: Vec<String>,
    /// `-L name`: the tmux server (here: a directory of session sockets) to talk to.
    pub socket_name: Option<String>,
    /// `-S path`: a socket path naming the server.
    pub socket_path: Option<String>,
}

impl Invocation {
    pub fn has(&self, flag: char) -> bool {
        self.switches.contains(&flag)
    }

    pub fn value(&self, flag: char) -> Option<&str> {
        self.values.get(&flag).map(String::as_str)
    }

    /// Operands joined by spaces, the way tmux hands a trailing command to `sh -c`.
    pub fn operand_text(&self) -> String {
        self.operands.join(" ")
    }
}

/// Canonical tmux command name for a name or alias.
pub fn canonical(name: &str) -> &str {
    match name {
        "a" | "at" | "attach" => "attach-session",
        "ls" => "list-sessions",
        "lsw" => "list-windows",
        "lsp" | "list" => "list-panes",
        "neww" => "new-window",
        "splitw" => "split-window",
        "send" => "send-keys",
        "capturep" => "capture-pane",
        "display" => "display-message",
        "killp" => "kill-pane",
        "killw" => "kill-window",
        "selectp" => "select-pane",
        "selectl" => "select-layout",
        "selectw" => "select-window",
        "resizep" => "resize-pane",
        "swapp" => "swap-pane",
        "breakp" => "break-pane",
        "rotatew" => "rotate-window",
        "renamew" => "rename-window",
        "rename" => "rename-session",
        "joinp" => "join-pane",
        "swapw" => "swap-window",
        "movew" => "move-window",
        "pipep" => "pipe-pane",
        "has" => "has-session",
        "lastw" => "last-window",
        "lastp" => "last-pane",
        "lsk" => "list-keys",
        "next" => "next-window",
        "prev" => "previous-window",
        "detach" => "detach-client",
        "switchc" => "switch-client",
        "set" | "set-window-option" | "setw" => "set-option",
        "show" | "show-window-options" | "showw" => "show-options",
        "setenv" => "set-environment",
        "showenv" => "show-environment",
        "setb" => "set-buffer",
        "showb" => "show-buffer",
        "lsb" => "list-buffers",
        "deleteb" => "delete-buffer",
        "loadb" => "load-buffer",
        "saveb" => "save-buffer",
        "pasteb" => "paste-buffer",
        "run" => "run-shell",
        "if" => "if-shell",
        "wait" => "wait-for",
        "source" => "source-file",
        "bind" => "bind-key",
        "unbind" => "unbind-key",
        other => other,
    }
}

/// Switch flags and value flags accepted by a canonical command; `None` when the command is not modelled.
pub fn spec(command: &str) -> Option<(&'static str, &'static str)> {
    Some(match command {
        "attach-session" => ("d", "t"),
        "split-window" => ("hvdPbf", "tcFl"),
        "new-window" => ("dPkab", "ncFt"),
        "new-session" => ("dPA", "sncxyFt"),
        "kill-pane" | "kill-window" | "kill-session" => ("a", "t"),
        "send-keys" => ("lHRM", "t"),
        "capture-pane" => ("apeJqCM", "tSEb"),
        "list-panes" => ("as", "tF"),
        "list-windows" => ("a", "tF"),
        "list-sessions" => ("", "F"),
        "display-message" => ("pIv", "tF"),
        "has-session" | "rename-window" | "rename-session" | "last-window" => ("", "t"),
        "select-window" => ("lnpT", "t"),
        "next-window" | "previous-window" => ("a", "t"),
        "kill-server" => ("", ""),
        "detach-client" => ("aP", "Est"),
        "switch-client" => ("ElnprZ", "ctT"),
        "set-option" => ("aFgopqsuUw", "t"),
        "show-options" => ("AgHpqsvw", "t"),
        "set-environment" => ("Fhgru", "t"),
        "show-environment" => ("hgs", "t"),
        "set-buffer" => ("aw", "bnt"),
        "show-buffer" | "delete-buffer" => ("", "b"),
        "list-buffers" => ("", "Ff"),
        "load-buffer" => ("w", "bt"),
        "save-buffer" => ("a", "b"),
        "paste-buffer" => ("dpr", "sbt"),
        "run-shell" => ("bC", "dt"),
        "if-shell" => ("bF", "t"),
        "wait-for" => ("LSU", ""),
        "source-file" => ("nqFv", "t"),
        "bind-key" => ("nr", "TN"),
        "list-keys" => ("1aN", "FT"),
        "unbind-key" => ("anq", "T"),
        "set-hook" => ("agRuw", "t"),
        "show-hooks" => ("gpw", "t"),
        "join-pane" => ("bdhvf", "lst"),
        "swap-window" => ("d", "st"),
        "move-window" => ("abdkr", "st"),
        "pipe-pane" => ("IOo", "t"),
        "select-pane" => ("LRUDmMZel", "tT"),
        "select-layout" => ("nopE", "t"),
        "resize-pane" => ("LRUDMZ", "txyt"),
        "swap-pane" => ("dDUZ", "st"),
        "break-pane" => ("dP", "sFnt"),
        "last-pane" => ("deZ", "t"),
        "rotate-window" => ("DUZ", "t"),
        "find-window" => ("CNTrZ", "t"),
        "respawn-pane" | "respawn-window" => ("k", "ct"),
        _ => return None,
    })
}

/// Commands accepted and ignored so that tools configuring tmux keep working.
pub fn is_noop(command: &str) -> bool {
    matches!(command, "refresh-client")
}

/// Parses a tmux argv (without the program name). Option parsing stops at the first operand,
/// as getopt does, and `--` ends options explicitly.
pub fn parse(argv: &[String]) -> Result<Invocation, String> {
    let mut start = 0;
    let mut version = false;
    let (mut socket_name, mut socket_path) = (None, None);
    while let Some(tok) = argv
        .get(start)
        .filter(|t| t.len() > 1 && t.starts_with('-'))
    {
        match tok.as_str() {
            "-V" => version = true,
            "-L" => {
                socket_name = argv.get(start + 1).cloned();
                start += 1;
            }
            "-S" => {
                socket_path = argv.get(start + 1).cloned();
                start += 1;
            }
            "-f" => start += 1,
            _ => {}
        }
        start += 1;
    }
    if version {
        return Ok(Invocation {
            command: "-V".to_string(),
            ..Invocation::default()
        });
    }
    let name = match argv.get(start) {
        Some(s) => s.as_str(),
        None => "new-session",
    };
    let command = canonical(name).to_string();
    let mut inv = Invocation {
        command: command.clone(),
        socket_name,
        socket_path,
        ..Invocation::default()
    };
    let Some(flags) = spec(&command) else {
        inv.operands = argv[start + 1..].to_vec();
        return Ok(inv);
    };
    let mut i = start + 1;
    while let Some(tok) = argv.get(i) {
        if tok == "--" {
            i += 1;
            break;
        }
        if tok.len() < 2 || !tok.starts_with('-') {
            break;
        }
        let consumed_next = parse_cluster(tok, argv.get(i + 1), flags, &mut inv)?;
        i += if consumed_next { 2 } else { 1 };
    }
    inv.operands = argv.get(i..).unwrap_or_default().to_vec();
    Ok(inv)
}

/// Applies one `-abc` / `-tVALUE` / `-t VALUE` token; returns true when the next token was consumed.
fn parse_cluster(
    tok: &str,
    next: Option<&String>,
    (switches, values): (&str, &str),
    inv: &mut Invocation,
) -> Result<bool, String> {
    let body: Vec<char> = tok.chars().skip(1).collect();
    for (j, c) in body.iter().enumerate() {
        if values.contains(*c) {
            let attached: String = body[j + 1..].iter().collect();
            if !attached.is_empty() {
                inv.values.insert(*c, attached);
                return Ok(false);
            }
            let value = next.ok_or_else(|| format!("command requires an argument for -{c}"))?;
            inv.values.insert(*c, value.clone());
            return Ok(true);
        }
        if !switches.contains(*c) {
            return Err(format!("unknown option -- {c}"));
        }
        inv.switches.insert(*c);
    }
    Ok(false)
}
