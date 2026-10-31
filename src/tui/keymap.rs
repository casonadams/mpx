//! Key tables: default bindings, config overrides, runtime bindings and the resolved keymap.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use crossterm::event::KeyEvent;

use super::command::{Command, parse};
use super::copy::CopyAction;
use super::overlay::HelpEntry;
use crate::config::{Config, ModeKeys};
use crate::keys::KeySpec;

/// Bindings typed after the prefix: key (tmux notation) -> command line.
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("c", "new-window"),
    ("n", "next-window"),
    ("p", "previous-window"),
    ("^", "last-window"),
    (",", "command-prompt -F -I \"#W\" \"rename-window -- '%%'\""),
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
    ("E", "select-layout -E"),
    ("=", "choose-buffer"),
    ("#", "list-buffers"),
    ("~", "show-messages"),
    ("t", "clock-mode"),
    ("i", "display-message"),
    ("C-o", "rotate-window"),
    ("M-o", "rotate-window -D"),
    ("C-z", "suspend-client"),
    ("PageUp", "copy-mode -u"),
    ("M-1", "select-layout even-horizontal"),
    ("M-2", "select-layout even-vertical"),
    ("M-3", "select-layout main-horizontal"),
    ("M-4", "select-layout main-vertical"),
    ("M-5", "select-layout tiled"),
    ("M-6", "select-layout main-horizontal-mirrored"),
    ("M-7", "select-layout main-vertical-mirrored"),
    ("M-n", "next-window -a"),
    ("M-p", "previous-window -a"),
    ("C-Up", "resize-pane -U 1"),
    ("C-Down", "resize-pane -D 1"),
    ("C-Left", "resize-pane -L 1"),
    ("C-Right", "resize-pane -R 1"),
    ("M-Up", "resize-pane -U 5"),
    ("M-Down", "resize-pane -D 5"),
    ("M-Left", "resize-pane -L 5"),
    ("M-Right", "resize-pane -R 5"),
    ("[", "copy-mode"),
    ("]", "paste"),
    ("d", "detach-client"),
    ("q", "display-panes"),
    (":", "command-prompt"),
    ("r", "reload-config"),
    ("?", "palette help"),
    ("s", "palette sessions"),
    ("w", "palette sessions"),
    (
        "$",
        "command-prompt -F -I \"#S\" \"rename-session -- '%%'\"",
    ),
    (".", "command-prompt \"move-window -t '%%'\""),
    (
        "'",
        "command-prompt -T window-target -p index \"select-window -t ':%%'\"",
    ),
    ("(", "prev-session"),
    (")", "next-session"),
    ("C-Space", "palette"),
];

/// Bindings for keys pressed without the prefix: they never reach the pane.
const ROOT_BINDINGS: &[(&str, &str)] = &[
    ("C-Space", "palette"),
];

/// The copy-mode table of `mode-keys vi`: a `send-keys -X` command per key. mpx keeps its own
/// behavior for `w`, `b`, `y`, `Enter` and `Escape` (see `docs/KEYBINDINGS.md`).
const COPY_VI_BINDINGS: &[(&str, &str)] = &[
    ("h", "send-keys -X cursor-left"),
    ("Left", "send-keys -X cursor-left"),
    ("j", "send-keys -X cursor-down"),
    ("Down", "send-keys -X cursor-down"),
    ("k", "send-keys -X cursor-up"),
    ("Up", "send-keys -X cursor-up"),
    ("l", "send-keys -X cursor-right"),
    ("Right", "send-keys -X cursor-right"),
    ("0", "send-keys -X start-of-line"),
    ("^", "send-keys -X start-of-line"),
    ("$", "send-keys -X end-of-line"),
    ("w", "send-keys -X forward-four-columns"),
    ("b", "send-keys -X back-four-columns"),
    ("C-u", "send-keys -X page-up"),
    ("C-d", "send-keys -X page-down"),
    ("PageUp", "send-keys -X page-up"),
    ("PageDown", "send-keys -X page-down"),
    ("g", "send-keys -X history-top"),
    ("G", "send-keys -X history-bottom"),
    ("?", "send-keys -X search-prompt-backward"),
    ("/", "send-keys -X search-prompt-forward"),
    ("n", "send-keys -X search-again"),
    ("N", "send-keys -X search-reverse"),
    ("v", "send-keys -X toggle-selection"),
    ("V", "send-keys -X toggle-line-selection"),
    ("C-v", "send-keys -X rectangle-toggle"),
    ("y", "send-keys -X copy-selection-or-line-and-cancel"),
    ("Enter", "send-keys -X copy-selection-or-line-and-cancel"),
    ("Escape", "send-keys -X clear-selection-or-cancel"),
    ("q", "send-keys -X cancel"),
];

/// The copy-mode table of `mode-keys emacs`.
const COPY_EMACS_BINDINGS: &[(&str, &str)] = &[
    ("C-b", "send-keys -X cursor-left"),
    ("Left", "send-keys -X cursor-left"),
    ("C-f", "send-keys -X cursor-right"),
    ("Right", "send-keys -X cursor-right"),
    ("C-p", "send-keys -X cursor-up"),
    ("Up", "send-keys -X cursor-up"),
    ("C-n", "send-keys -X cursor-down"),
    ("Down", "send-keys -X cursor-down"),
    ("M-b", "send-keys -X back-four-columns"),
    ("M-f", "send-keys -X forward-four-columns"),
    ("C-a", "send-keys -X start-of-line"),
    ("Home", "send-keys -X start-of-line"),
    ("C-e", "send-keys -X end-of-line"),
    ("End", "send-keys -X end-of-line"),
    ("C-v", "send-keys -X page-down"),
    ("PageDown", "send-keys -X page-down"),
    ("M-v", "send-keys -X page-up"),
    ("PageUp", "send-keys -X page-up"),
    ("M-<", "send-keys -X history-top"),
    ("M->", "send-keys -X history-bottom"),
    ("C-Space", "send-keys -X toggle-selection"),
    ("C-w", "send-keys -X copy-selection-and-cancel"),
    ("M-w", "send-keys -X copy-selection-and-cancel"),
    ("Enter", "send-keys -X copy-selection-if-any"),
    ("C-g", "send-keys -X cancel"),
    ("Escape", "send-keys -X cancel"),
    ("q", "send-keys -X cancel"),
    ("C-s", "send-keys -X search-prompt-forward"),
    ("C-r", "send-keys -X search-prompt-backward"),
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

/// A key table: the prefix table, the root table (keys without the prefix), or one of the two
/// copy-mode tables (`copy-mode-vi` for `mode-keys vi`, `copy-mode` for `mode-keys emacs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Prefix,
    Root,
    CopyVi,
    CopyEmacs,
}

impl Table {
    /// tmux's name for the table, as `bind-key -T` and `list-keys -T` spell it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Prefix => "prefix",
            Self::Root => "root",
            Self::CopyVi => "copy-mode-vi",
            Self::CopyEmacs => "copy-mode",
        }
    }

    /// The table `name` names, or `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Prefix, Self::Root, Self::CopyVi, Self::CopyEmacs]
            .into_iter()
            .find(|table| table.name() == name)
    }

    pub fn is_copy(self) -> bool {
        matches!(self, Self::CopyVi | Self::CopyEmacs)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    prefix: KeySpec,
    bindings: Vec<Binding>,
    /// Keys bound without the prefix.
    root: Vec<Binding>,
    /// Copy-mode keys of `mode-keys vi`.
    copy_vi: Vec<Binding>,
    /// Copy-mode keys of `mode-keys emacs`.
    copy_emacs: Vec<Binding>,
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
        self.rows(table).iter().find(|b| b.key.matches(ev))
    }

    fn rows(&self, table: Table) -> &[Binding] {
        match table {
            Table::Prefix => &self.bindings,
            Table::Root => &self.root,
            Table::CopyVi => &self.copy_vi,
            Table::CopyEmacs => &self.copy_emacs,
        }
    }

    /// The copy-mode command `ev` runs in the table `keys` selects (`mode-keys`), if any.
    pub fn copy_action(&self, keys: ModeKeys, ev: &KeyEvent) -> Option<CopyAction> {
        let table = match keys {
            ModeKeys::Vi => Table::CopyVi,
            ModeKeys::Emacs => Table::CopyEmacs,
        };
        match &self.lookup(table, ev)?.command {
            Command::CopyCommand { action, .. } => Some(action.clone()),
            _ => None,
        }
    }

    /// Binds `key` (tmux notation) to `line` in `table`, replacing an earlier binding of the key;
    /// an empty `line` removes it. The prefix key itself stays bound to `send-prefix`. The copy
    /// tables take only `send-keys -X` commands.
    pub fn bind(&mut self, table: Table, key: &str, line: &str) -> Result<(), String> {
        let spec = parse_key(key)?;
        if table == Table::Prefix && spec == self.prefix {
            return Err(format!("{key} is the prefix key"));
        }
        let new = if line.trim().is_empty() {
            None
        } else {
            Some(binding(spec, line.to_string())?)
        };
        if let Some(b) = &new
            && table.is_copy()
            && !matches!(b.command, Command::CopyCommand { .. })
        {
            return Err(format!("{}: only send-keys -X commands can be bound", table.name()));
        }
        let rows = match table {
            Table::Prefix => &mut self.bindings,
            Table::Root => &mut self.root,
            Table::CopyVi => &mut self.copy_vi,
            Table::CopyEmacs => &mut self.copy_emacs,
        };
        rows.retain(|b| b.key != spec);
        rows.extend(new);
        Ok(())
    }

    /// Every binding as (table, key in tmux notation, command line).
    pub fn entries(&self) -> Vec<(Table, String, String)> {
        [Table::Prefix, Table::Root, Table::CopyVi, Table::CopyEmacs]
            .into_iter()
            .flat_map(|table| {
                self.rows(table)
                    .iter()
                    .map(move |b| (table, b.key.display(), b.source.clone()))
            })
            .collect()
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
    let copy = |rows: &[(&str, &str)]| -> Result<Vec<Binding>, String> {
        rows.iter()
            .map(|(key, line)| binding(parse_key(key)?, line.to_string()))
            .collect()
    };
    Ok(Keymap {
        prefix,
        bindings,
        root,
        copy_vi: copy(COPY_VI_BINDINGS)?,
        copy_emacs: copy(COPY_EMACS_BINDINGS)?,
    })
}

/// The copy-mode command `ev` runs with the default table of `keys`, without any user binding.
pub fn default_copy_action(keys: ModeKeys, ev: &KeyEvent) -> Option<CopyAction> {
    static DEFAULTS: LazyLock<Keymap> =
        LazyLock::new(|| resolve(&Config::default()).expect("default key tables resolve"));
    DEFAULTS.copy_action(keys, ev)
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
