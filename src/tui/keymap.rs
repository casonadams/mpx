//! Prefix-key table: default bindings, config overrides and the resolved keymap.
use crossterm::event::KeyEvent;

use super::command::{Command, parse};
use super::overlay::HelpEntry;
use crate::config::Config;
use crate::keys::KeySpec;

/// Bindings typed after the prefix: key (tmux notation), command line, label in the shortcut popup.
const DEFAULT_BINDINGS: &[(&str, &str, &str)] = &[
    ("c", "new-window", "new window"),
    ("n", "next-window", "next window"),
    ("p", "previous-window", "previous window"),
    ("^", "last-window", "last window"),
    (",", "command-prompt -I 'rename-window '", "rename window"),
    ("&", "kill-window", "kill window"),
    ("%", "split-window -h", "split side by side"),
    ("|", "split-window -h", "split side by side"),
    ("\"", "split-window", "split stacked"),
    ("-", "split-window", "split stacked"),
    ("h", "select-pane -L", "focus left"),
    ("Left", "select-pane -L", "focus left"),
    ("j", "select-pane -D", "focus down"),
    ("Down", "select-pane -D", "focus down"),
    ("k", "select-pane -U", "focus up"),
    ("Up", "select-pane -U", "focus up"),
    ("l", "select-pane -R", "focus right"),
    ("Right", "select-pane -R", "focus right"),
    ("o", "next-pane", "next pane"),
    ("Tab", "next-pane", "next pane"),
    (";", "last-pane", "last pane"),
    ("H", "resize-pane -L", "resize left"),
    ("J", "resize-pane -D", "resize down"),
    ("K", "resize-pane -U", "resize up"),
    ("L", "resize-pane -R", "resize right"),
    ("z", "resize-pane -Z", "zoom pane"),
    ("x", "kill-pane", "kill pane"),
    ("!", "break-pane", "pane to new window"),
    ("{", "swap-pane -U", "swap pane up"),
    ("}", "swap-pane -D", "swap pane down"),
    ("Space", "next-layout", "next layout"),
    ("[", "copy-mode", "copy mode"),
    ("d", "detach-client", "detach"),
    ("q", "detach-client", "detach"),
    (":", "command-prompt", "command prompt"),
    ("?", "list-keys", "all key bindings"),
    ("s", "choose-tree", "session tree"),
    ("w", "choose-tree", "session tree"),
    ("(", "prev-session", "previous session"),
    (")", "next-session", "next session"),
    ("a", "agents", "agent manager"),
    ("A", "next-attention", "next agent needing you"),
];

/// The default table: `DEFAULT_BINDINGS` plus `0`-`9` for `select-window`.
pub fn default_table() -> Vec<(String, String)> {
    let mut table: Vec<(String, String)> = DEFAULT_BINDINGS
        .iter()
        .map(|(key, line, _)| (key.to_string(), line.to_string()))
        .collect();
    table.extend((0..=9).map(|d| (d.to_string(), format!("select-window -t {d}"))));
    table
}

/// One row of the shortcut popup: the keys bound to one command and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: String,
    pub label: String,
}

/// Groups the `0`-`9` window bindings into one popup row.
const WINDOW_DIGITS: &str = "select-window -t N";

fn shortcut_group(key: &str, line: &str) -> String {
    let digit_window = key.len() == 1
        && key.chars().all(|c| c.is_ascii_digit())
        && line == format!("select-window -t {key}");
    if digit_window {
        WINDOW_DIGITS.to_string()
    } else {
        line.to_string()
    }
}

/// What a command line does, in words: the default label for known commands, else the line itself.
fn shortcut_label(group: &str) -> String {
    let known = match group {
        WINDOW_DIGITS => Some("select window"),
        "send-prefix" => Some("send prefix to pane"),
        _ => DEFAULT_BINDINGS
            .iter()
            .find(|(_, line, _)| *line == group)
            .map(|(_, _, label)| *label),
    };
    known.map_or_else(|| group.to_string(), str::to_string)
}

fn shortcut_keys(group: &str, keys: &[String]) -> String {
    match (group, keys.first(), keys.last()) {
        (WINDOW_DIGITS, Some(first), Some(last)) if keys.len() > 1 => format!("{first}-{last}"),
        _ => keys.join(" "),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub key: KeySpec,
    pub source: String,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    prefix: KeySpec,
    bindings: Vec<Binding>,
}

impl Keymap {
    pub fn prefix(&self) -> &KeySpec {
        &self.prefix
    }

    pub fn is_prefix(&self, ev: &KeyEvent) -> bool {
        self.prefix.matches(ev)
    }

    pub fn lookup(&self, ev: &KeyEvent) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.key.matches(ev))
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn help_entries(&self) -> Vec<HelpEntry> {
        self.bindings
            .iter()
            .map(|b| HelpEntry {
                key: b.key.display(),
                command: b.source.clone(),
            })
            .collect()
    }

    /// Every binding for the shortcut popup, in table order. Keys bound to the same command share a
    /// row, and the `0`-`9` window keys collapse into one.
    pub fn shortcuts(&self) -> Vec<Shortcut> {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for b in &self.bindings {
            let key = b.key.display();
            let group = shortcut_group(&key, &b.source);
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, keys)) => keys.push(key),
                None => groups.push((group, vec![key])),
            }
        }
        groups
            .into_iter()
            .map(|(group, keys)| Shortcut {
                keys: shortcut_keys(&group, &keys),
                label: shortcut_label(&group),
            })
            .collect()
    }
}

/// Resolves the prefix and bindings: defaults, then `config.bind` replacing or adding keys.
/// The prefix key itself is bound to `send-prefix`.
pub fn resolve(config: &Config) -> Result<Keymap, String> {
    let prefix = parse_key(&config.prefix)?;
    let mut table: Vec<(KeySpec, String)> = Vec::new();
    for (key, line) in default_table() {
        table.push((parse_key(&key)?, line));
    }
    for (key, line) in &config.bind {
        let spec = parse_key(key)?;
        table.retain(|(existing, _)| *existing != spec);
        table.push((spec, line.clone()));
    }
    let mut bindings = vec![binding(prefix, "send-prefix".to_string())?];
    for (key, line) in table {
        bindings.push(binding(key, line)?);
    }
    Ok(Keymap { prefix, bindings })
}

fn parse_key(spec: &str) -> Result<KeySpec, String> {
    KeySpec::parse(spec).map_err(|e| format!("invalid key '{spec}': {e}"))
}

fn binding(key: KeySpec, source: String) -> Result<Binding, String> {
    let command = parse(&source).map_err(|e| format!("bind {}: {e}", key.display()))?;
    Ok(Binding {
        key,
        source,
        command,
    })
}
