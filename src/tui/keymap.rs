//! Prefix-key table: default bindings, config overrides and the resolved keymap.
use std::collections::BTreeMap;

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
    ("m", "select-pane -m"),
    ("M", "select-pane -M"),
    ("{", "swap-pane -U"),
    ("}", "swap-pane -D"),
    ("Space", "next-layout"),
    ("=", "select-layout -E"),
    ("[", "copy-mode"),
    ("d", "detach-client"),
    ("q", "detach-client"),
    (":", "command-prompt"),
    ("?", "palette help"),
    ("s", "palette agents"),
    ("w", "palette sessions"),
    (".", "sidebar"),
    ("(", "prev-session"),
    (")", "next-session"),
    ("C-Space", "palette"),
];

/// Bindings for keys pressed without the prefix: they never reach the pane.
const ROOT_BINDINGS: &[(&str, &str)] = &[
    ("C-Space", "palette"),
];

/// What macOS terminals send for Option-j and Option-k (US and ABC layouts) unless Option is set
/// to act as Alt (Meta / Esc+), so the same keys step through agents out of the box.
#[cfg(target_os = "macos")]
const MAC_OPTION_BINDINGS: &[(&str, &str)] = &[];
#[cfg(not(target_os = "macos"))]
const MAC_OPTION_BINDINGS: &[(&str, &str)] = &[];

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

/// Where a key is looked up: typed after the prefix, or without it (tmux's root table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Prefix,
    Root,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    prefix: KeySpec,
    bindings: Vec<Binding>,
    /// Keys bound without the prefix.
    root: Vec<Binding>,
}

impl Keymap {
    pub fn prefix(&self) -> &KeySpec {
        &self.prefix
    }

    pub fn is_prefix(&self, ev: &KeyEvent) -> bool {
        self.prefix.matches(ev)
    }

    /// The binding of `ev` in `table`, if it has one.
    pub fn lookup(&self, table: Table, ev: &KeyEvent) -> Option<&Binding> {
        let bindings = match table {
            Table::Prefix => &self.bindings,
            Table::Root => &self.root,
        };
        bindings.iter().find(|b| b.key.matches(ev))
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn help_entries(&self) -> Vec<HelpEntry> {
        let entry = |b: &Binding, key: String| HelpEntry {
            key,
            command: b.source.clone(),
        };
        let prefixed = self.bindings.iter().map(|b| entry(b, b.key.display()));
        let root = (self.root.iter()).map(|b| entry(b, format!("{} (no prefix)", b.key.display())));
        prefixed.chain(root).collect()
    }

}

/// Resolves the prefix and bindings: defaults, then `config.bind` replacing or adding keys.
/// The prefix key itself is bound to `send-prefix`.
pub fn resolve(config: &Config) -> Result<Keymap, String> {
    let prefix = parse_key(&config.prefix)?;
    let mut bindings = vec![binding(prefix, "send-prefix".to_string())?];
    for (key, line) in merged(default_table(), &config.bind)? {
        bindings.push(binding(key, line)?);
    }
    let root_defaults = ROOT_BINDINGS
        .iter()
        .chain(MAC_OPTION_BINDINGS)
        .map(|(key, line)| (key.to_string(), line.to_string()));
    let root = merged(root_defaults, &config.bind_root)?
        .into_iter()
        .map(|(key, line)| binding(key, line))
        .collect::<Result<_, _>>()?;
    Ok(Keymap {
        prefix,
        bindings,
        root,
    })
}

/// `defaults` with `overrides` replacing or adding keys; an empty command line unbinds a key.
fn merged(
    defaults: impl IntoIterator<Item = (String, String)>,
    overrides: &BTreeMap<String, String>,
) -> Result<Vec<(KeySpec, String)>, String> {
    let mut table: Vec<(KeySpec, String)> = Vec::new();
    for (key, line) in defaults {
        table.push((parse_key(&key)?, line));
    }
    for (key, line) in overrides {
        let spec = parse_key(key)?;
        table.retain(|(existing, _)| *existing != spec);
        if !line.trim().is_empty() {
            table.push((spec, line.clone()));
        }
    }
    Ok(table)
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
