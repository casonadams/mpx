//! Prefix-key table: default bindings, config overrides and the resolved keymap.
use crossterm::event::KeyEvent;

use super::command::{Command, parse};
use super::overlay::HelpEntry;
use crate::config::Config;
use crate::keys::KeySpec;

/// Bindings typed after the prefix: key (tmux notation) -> command line.
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("c", "new-window"),
    ("n", "next-window"),
    ("p", "previous-window"),
    ("^", "last-window"),
    (",", "command-prompt -I 'rename-window '"),
    ("&", "kill-window"),
    ("%", "split-window -h"),
    ("|", "split-window -h"),
    ("\"", "split-window"),
    ("-", "split-window"),
    ("h", "select-pane -L"),
    ("Left", "select-pane -L"),
    ("j", "select-pane -D"),
    ("Down", "select-pane -D"),
    ("k", "select-pane -U"),
    ("Up", "select-pane -U"),
    ("l", "select-pane -R"),
    ("Right", "select-pane -R"),
    ("o", "next-pane"),
    ("Tab", "next-pane"),
    (";", "last-pane"),
    ("H", "resize-pane -L"),
    ("J", "resize-pane -D"),
    ("K", "resize-pane -U"),
    ("L", "resize-pane -R"),
    ("z", "resize-pane -Z"),
    ("x", "kill-pane"),
    ("!", "break-pane"),
    ("{", "swap-pane -U"),
    ("}", "swap-pane -D"),
    ("Space", "next-layout"),
    ("[", "copy-mode"),
    ("d", "detach-client"),
    ("q", "detach-client"),
    (":", "command-prompt"),
    ("?", "list-keys"),
    ("s", "agents"),
    ("w", "choose-tree"),
    ("(", "prev-session"),
    (")", "next-session"),
    ("A", "next-attention"),
];

/// The default table: `DEFAULT_BINDINGS` plus `0`-`9` for `select-window`.
pub fn default_table() -> Vec<(String, String)> {
    let mut table: Vec<(String, String)> = DEFAULT_BINDINGS
        .iter()
        .map(|(key, line)| (key.to_string(), line.to_string()))
        .collect();
    table.extend((0..=9).map(|d| (d.to_string(), format!("select-window -t {d}"))));
    table
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
