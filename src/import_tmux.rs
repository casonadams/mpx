//! `mpx import-tmux`: converts a tmux config into an mpx `config.toml` fragment. Pure: the caller reads the file.
use std::collections::BTreeMap;

use crate::keys::KeySpec;
use crate::tui::command::parse as parse_command;
use crate::tui::render::parse_color;

/// The converted fragment and one note per skipped or approximated line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Converted {
    /// TOML that `Config::parse` accepts. Skipped lines come first as `# unsupported (line N): ...` comments.
    pub toml: String,
    /// `line N: ...` for each skipped or approximated line, in file order.
    pub notes: Vec<String>,
}

/// Converts the text of a tmux config. Lines mpx cannot express are skipped and noted, never guessed.
pub fn convert(text: &str) -> Converted {
    let mut conv = Conv::default();
    let mut skipping = 0usize;
    for (line, src) in logical_lines(text) {
        let head = src.split_whitespace().next().unwrap_or("");
        if skipping > 0 {
            match head {
                "%if" => skipping += 1,
                "%endif" => skipping -= 1,
                _ => {}
            }
        } else if head == "%if" {
            skipping = 1;
            conv.note(line, "%if block skipped: conditionals are not evaluated");
        } else if head.starts_with('%') {
            conv.note(line, format!("'{head}' is not supported"));
        } else {
            conv.line(line, &src);
        }
    }
    let notes = conv
        .notes
        .iter()
        .map(|(line, msg)| format!("line {line}: {msg}"))
        .collect();
    Converted {
        toml: conv.render(),
        notes,
    }
}

#[derive(Default)]
struct Conv {
    notes: Vec<(usize, String)>,
    top: BTreeMap<&'static str, String>,
    style: BTreeMap<&'static str, String>,
    /// Prefix-table bindings: key -> command line (empty removes mpx's default).
    bind: BTreeMap<String, String>,
    /// Root-table bindings (`bind -n`).
    root: BTreeMap<String, String>,
}

impl Conv {
    fn note(&mut self, line: usize, msg: impl Into<String>) {
        self.notes.push((line, msg.into()));
    }

    fn table(&mut self, root: bool) -> &mut BTreeMap<String, String> {
        if root { &mut self.root } else { &mut self.bind }
    }

    fn line(&mut self, line: usize, src: &str) {
        match tokenize(src) {
            Ok(commands) => {
                for words in commands {
                    self.command(line, &words);
                }
            }
            Err(e) => self.note(line, format!("cannot parse line ({e})")),
        }
    }

    fn command(&mut self, line: usize, words: &[String]) {
        let Some((name, args)) = words.split_first() else {
            return;
        };
        match name.as_str() {
            "bind" | "bind-key" => self.bind(line, args),
            "unbind" | "unbind-key" => self.unbind(line, args),
            "set" | "set-option" | "setw" | "set-window-option" => self.set(line, args),
            "source" | "source-file" => self.note(
                line,
                "source-file is not followed: only this file is converted",
            ),
            "run" | "run-shell" => {
                self.note(line, format!("'{name}': plugin/shell commands are not run"))
            }
            other => self.note(line, format!("command '{other}' has no mpx equivalent")),
        }
    }

    fn bind(&mut self, line: usize, args: &[String]) {
        let opts = match Opts::parse(args, &['T', 'N']) {
            Ok(opts) => opts,
            Err(e) => return self.note(line, format!("bind: {e}")),
        };
        if let Some(flag) = opts.flags.iter().find(|f| !matches!(f, 'n' | 'r')) {
            return self.note(line, format!("bind: option -{flag} is not supported"));
        }
        let Some((key, command)) = opts.rest.split_first() else {
            return self.note(line, "bind: missing key");
        };
        let root = match root_table(&opts) {
            Ok(root) => root,
            Err(e) => return self.note(line, format!("bind {key}: {e}")),
        };
        if let Err(e) = KeySpec::parse(key) {
            return self.note(line, format!("bind {key}: invalid key ({e})"));
        }
        match mpx_command(command) {
            Ok((cmd, dropped)) => {
                if opts.has('r') {
                    self.note(line, format!("bind {key}: -r (repeat) is ignored"));
                }
                if let Some(dropped) = dropped {
                    self.note(line, format!("bind {key}: {dropped}"));
                }
                self.table(root).insert(key.clone(), cmd);
            }
            Err(e) => self.note(line, format!("bind {key}: {e}")),
        }
    }

    fn unbind(&mut self, line: usize, args: &[String]) {
        let opts = match Opts::parse(args, &['T']) {
            Ok(opts) => opts,
            Err(e) => return self.note(line, format!("unbind: {e}")),
        };
        if opts.has('a') {
            return self.note(line, "unbind -a: unbinding every key is not supported");
        }
        let root = match root_table(&opts) {
            Ok(root) => root,
            Err(e) => return self.note(line, format!("unbind: {e}")),
        };
        let Some(key) = opts.rest.first() else {
            return self.note(line, "unbind: missing key");
        };
        if let Err(e) = KeySpec::parse(key) {
            return self.note(line, format!("unbind {key}: invalid key ({e})"));
        }
        self.table(root).insert(key.clone(), String::new());
        self.note(
            line,
            format!("unbind {key}: written as an empty binding, which removes mpx's default for that key"),
        );
    }

    fn set(&mut self, line: usize, args: &[String]) {
        let opts = match Opts::parse(args, &['t']) {
            Ok(opts) => opts,
            Err(e) => return self.note(line, format!("set: {e}")),
        };
        if opts.has('a') || opts.has('u') {
            return self.note(
                line,
                "set -a/-u: appending or unsetting options is not supported",
            );
        }
        if opts.value('t').is_some() {
            return self.note(line, "set -t: targeted options are not supported");
        }
        let Some((name, values)) = opts.rest.split_first() else {
            return self.note(line, "set: missing option name");
        };
        if name.starts_with('@') {
            return self.note(line, format!("user option '{name}' has no mpx equivalent"));
        }
        if opts.has('w') || opts.has('p') {
            self.note(
                line,
                format!("set '{name}': window/pane scope is applied to the whole mpx config"),
            );
        }
        self.option(line, name, &values.join(" "));
    }

    fn option(&mut self, line: usize, name: &str, value: &str) {
        if value.is_empty() {
            return self.note(line, format!("set {name}: missing value"));
        }
        match name {
            "prefix" => match KeySpec::parse(value) {
                Ok(_) => {
                    self.top.insert("prefix", toml_str(value));
                }
                Err(e) => self.note(line, format!("prefix '{value}' is not a valid key ({e})")),
            },
            "prefix2" => self.note(line, "prefix2 is not supported: mpx has one prefix key"),
            "history-limit" => match value.parse::<usize>() {
                Ok(n) => {
                    self.top.insert("scrollback_lines", n.to_string());
                }
                Err(_) => self.note(line, format!("history-limit '{value}' is not a number")),
            },
            "remain-on-exit" => match value.parse::<crate::config::RemainOnExit>() {
                Ok(mode) => {
                    self.top.insert("remain_on_exit", toml_str(mode.as_str()));
                }
                Err(e) => self.note(line, format!("remain-on-exit: {e}")),
            },
            "allow-passthrough" => match value.parse::<crate::config::Passthrough>() {
                Ok(mode) => {
                    self.top.insert("allow_passthrough", toml_str(mode.as_str()));
                }
                Err(e) => self.note(line, format!("allow-passthrough: {e}")),
            },
            "automatic-rename" => self.flag(line, name, "automatic_rename", value),
            "monitor-activity" => self.flag(line, name, "monitor_activity", value),
            "monitor-bell" => self.flag(line, name, "monitor_bell", value),
            "monitor-silence" => match value.parse::<usize>() {
                Ok(0) => {}
                _ => self.note(line, "monitor-silence: not supported by mpx; ignored"),
            },
            "display-panes-time" => match value.parse::<u64>() {
                Ok(ms) => {
                    self.top.insert("display_panes_time_ms", ms.to_string());
                }
                Err(_) => self.note(line, format!("display-panes-time '{value}' is not a number")),
            },
            "default-terminal" => {
                self.top.insert("default_terminal", toml_str(value));
            }
            "default-shell" if value.starts_with('/') => {
                self.top.insert("default_shell", toml_str(value));
            }
            "default-shell" => self.note(line, "default-shell must be an absolute path; ignored"),
            "default-command" => {
                self.top.insert("default_command", toml_str(value));
            }
            "status-style" => self.colors(
                line,
                name,
                value,
                &[("bg", "status_bg"), ("fg", "status_fg")],
            ),
            "pane-border-style" => {
                self.colors(line, name, value, &[("fg", "border_inactive")]);
            }
            "pane-active-border-style" => {
                self.colors(line, name, value, &[("fg", "border_active")]);
            }
            "pane-border-lines" => {
                let style = match value {
                    "single" | "double" | "rounded" => value,
                    "simple" => {
                        self.note(line, "pane-border-lines simple: approximated as ascii");
                        "ascii"
                    }
                    _ => {
                        return self.note(
                            line,
                            format!("pane-border-lines '{value}' has no mpx equivalent; ignored"),
                        );
                    }
                };
                self.style.insert("border_style", toml_str(style));
            }
            "mouse" => self.flag(line, name, "mouse", value),
            "mode-keys" => match value {
                "vi" | "emacs" => {
                    self.top.insert("mode_keys", toml_str(value));
                }
                _ => self.note(line, format!("mode-keys '{value}' is not vi or emacs")),
            },
            _ => self.note(
                line,
                format!("option '{name}' has no mpx equivalent; ignored"),
            ),
        }
    }

    fn flag(&mut self, line: usize, name: &str, key: &'static str, value: &str) {
        match on_off(value) {
            Some(on) => {
                self.top.insert(key, on.to_string());
            }
            None => self.note(line, format!("{name} '{value}' is not on or off")),
        }
    }

    /// Maps the `fg=`/`bg=` items of a tmux style string onto mpx style keys.
    fn colors(&mut self, line: usize, name: &str, value: &str, targets: &[(&str, &'static str)]) {
        for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            let target = item.split_once('=').and_then(|(k, v)| {
                targets
                    .iter()
                    .find(|(t, _)| *t == k.trim())
                    .map(|(_, key)| (*key, v.trim()))
            });
            match target {
                Some((_, "default")) => self.note(
                    line,
                    format!("{name} '{item}': 'default' has no mpx equivalent; ignored"),
                ),
                Some((key, color)) if parse_color(color).is_some() => {
                    self.style.insert(key, toml_str(color));
                }
                Some((_, color)) => {
                    self.note(line, format!("{name} '{item}': '{color}' is not a colour"))
                }
                None => self.note(
                    line,
                    format!("{name} '{item}' has no mpx equivalent; ignored"),
                ),
            }
        }
    }

    fn render(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();
        if !self.notes.is_empty() {
            blocks.push(
                self.notes
                    .iter()
                    .map(|(line, msg)| format!("# unsupported (line {line}): {}\n", comment(msg)))
                    .collect(),
            );
        }
        if !self.top.is_empty() {
            blocks.push(
                self.top
                    .iter()
                    .map(|(key, value)| format!("{key} = {value}\n"))
                    .collect(),
            );
        }
        if !self.style.is_empty() {
            let body: String = self
                .style
                .iter()
                .map(|(key, value)| format!("{key} = {value}\n"))
                .collect();
            blocks.push(format!("[style]\n{body}"));
        }
        for (table, name) in [(&self.bind, "bind"), (&self.root, "bind_root")] {
            if !table.is_empty() {
                let body: String = table
                    .iter()
                    .map(|(key, cmd)| format!("{} = {}\n", toml_str(key), toml_str(cmd)))
                    .collect();
                blocks.push(format!("[{name}]\n{body}"));
            }
        }
        blocks.join("\n")
    }
}

/// Whether a `bind`/`unbind` line targets the root table (`-n` or `-T root`) rather than the prefix table.
fn root_table(opts: &Opts) -> Result<bool, String> {
    if opts.has('n') {
        return Ok(true);
    }
    match opts.value('T') {
        None | Some("prefix") => Ok(false),
        Some("root") => Ok(true),
        Some(table) => Err(format!("key table '{table}' is not supported")),
    }
}

/// The mpx command line for a tmux command, plus a note when an option had to be dropped.
fn mpx_command(words: &[String]) -> Result<(String, Option<String>), String> {
    let Some(name) = words.first() else {
        return Err("missing command".to_string());
    };
    let takes_cwd = matches!(
        name.as_str(),
        "split-window" | "split" | "sp" | "vsplit" | "vs" | "new-window" | "new" | "tabnew"
    );
    let mut kept: Vec<String> = Vec::with_capacity(words.len());
    let mut dropped = None;
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        if takes_cwd && word == "-c" {
            let Some(dir) = iter.next() else {
                return Err(format!("command '{name}': -c needs a value"));
            };
            if dir == "#{pane_current_path}" {
                dropped = Some(
                    "-c '#{pane_current_path}' dropped: mpx starts new panes in the active pane's directory"
                        .to_string(),
                );
            } else if dir.contains("#{") {
                return Err(format!(
                    "command '{name}': -c '{dir}' format is not supported"
                ));
            } else {
                kept.push(word.clone());
                kept.push(dir.clone());
            }
        } else {
            kept.push(word.clone());
        }
    }
    let line = crate::cmdline::join_words(&kept);
    parse_command(&line).map_err(|e| format!("command '{name}' not supported ({e})"))?;
    Ok((line, dropped))
}

/// tmux-style options: bundled switches (`-nr`), options taking a value (`-T root`), then operands.
/// Parsing stops at the first word that is not an option.
struct Opts {
    flags: Vec<char>,
    values: std::collections::BTreeMap<char, String>,
    rest: Vec<String>,
}

impl Opts {
    fn parse(args: &[String], valued: &[char]) -> Result<Self, String> {
        let mut opts = Opts {
            flags: Vec::new(),
            values: std::collections::BTreeMap::new(),
            rest: Vec::new(),
        };
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            i += 1;
            if arg == "--" {
                opts.rest.extend_from_slice(&args[i..]);
                break;
            }
            let Some(bundle) = arg.strip_prefix('-').filter(|b| !b.is_empty()) else {
                opts.rest.extend_from_slice(&args[i - 1..]);
                break;
            };
            for c in bundle.chars() {
                if valued.contains(&c) {
                    let value = args
                        .get(i)
                        .ok_or_else(|| format!("option -{c} needs a value"))?;
                    i += 1;
                    opts.values.insert(c, value.clone());
                } else {
                    opts.flags.push(c);
                }
            }
        }
        Ok(opts)
    }

    fn has(&self, flag: char) -> bool {
        self.flags.contains(&flag)
    }

    /// The last value given for `flag`.
    fn value(&self, flag: char) -> Option<&str> {
        self.values.get(&flag).map(String::as_str)
    }
}

/// Physical lines joined across trailing `\` continuations, each tagged with its first physical line
/// number. Blank and `#` comment lines are dropped.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (idx, raw) in text.lines().enumerate() {
        let (start, mut acc, piece) = match open.take() {
            Some((start, acc)) => (start, acc, raw),
            None => {
                let piece = raw.trim_start();
                if piece.is_empty() || piece.starts_with('#') {
                    continue;
                }
                (idx + 1, String::new(), piece)
            }
        };
        acc.push_str(piece);
        if acc.ends_with('\\') {
            acc.pop();
            open = Some((start, acc));
        } else {
            out.push((start, acc));
        }
    }
    if let Some(pending) = open {
        out.push(pending);
    }
    out
}

/// Splits one logical line into tmux commands at unquoted `;`. Quotes group words, `\x` escapes `x`
/// (so `\;` is a literal `;` word, as bind chains use), and `;` may follow a word directly.
fn tokenize(line: &str) -> Result<Vec<Vec<String>>, String> {
    let mut commands: Vec<Vec<String>> = vec![Vec::new()];
    let mut word: Option<String> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => w.push(ch),
                        None => return Err("unterminated single quote".to_string()),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            w.push(chars.next().unwrap_or('\\'));
                        }
                        Some(ch) => w.push(ch),
                        None => return Err("unterminated double quote".to_string()),
                    }
                }
            }
            '\\' => word
                .get_or_insert_with(String::new)
                .push(chars.next().unwrap_or('\\')),
            ';' => {
                end_word(&mut commands, &mut word);
                commands.push(Vec::new());
            }
            c if c.is_whitespace() => end_word(&mut commands, &mut word),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    end_word(&mut commands, &mut word);
    commands.retain(|words| !words.is_empty());
    Ok(commands)
}

fn end_word(commands: &mut [Vec<String>], word: &mut Option<String>) {
    if let (Some(w), Some(current)) = (word.take(), commands.last_mut()) {
        current.push(w);
    }
}

fn on_off(value: &str) -> Option<bool> {
    match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A TOML basic string literal for `s`.
fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Text safe inside a TOML comment line.
fn comment(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
