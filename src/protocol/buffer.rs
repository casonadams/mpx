use serde::{Deserialize, Serialize};

/// A paste buffer: tmux's stack of copied text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Buffer {
    pub name: String,
    pub data: String,
}

/// One variable of the session environment: `Some(value)` is set, `None` marks a variable that is
/// kept out of new panes (`set-environment -r`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub name: String,
    pub value: Option<String>,
}

/// How `set-environment` changes one variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvChange {
    Set(String),
    /// Drops the variable from the session environment (`set-environment -u`).
    Unset,
    /// Keeps the variable out of new panes, even one mpx inherited (`set-environment -r`).
    Remove,
}

/// Options of `paste-buffer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasteBufferOpts {
    /// The buffer to paste; the newest one when `None`.
    pub name: Option<String>,
    /// The pane to paste into.
    pub pane: String,
    /// Deletes the buffer after pasting (`-d`).
    #[serde(default)]
    pub delete: bool,
    /// Brackets the paste even when the program did not ask for bracketed paste (`-p`).
    #[serde(default)]
    pub bracketed: bool,
    /// Line feeds are pasted as they are, not as carriage returns (`-r`).
    #[serde(default)]
    pub raw: bool,
    /// Replaces each line feed with this text (`-s`).
    pub separator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum BufferRequest {
    /// Adds a paste buffer: `name` replaces the buffer of that name, else a new `bufferNNNN` one.
    /// With `append` (`set-buffer -a`), `data` is added to the end of the named buffer (the newest
    /// one when unnamed), or starts a buffer when there is none.
    Set {
        name: Option<String>,
        data: String,
        #[serde(default)]
        append: bool,
    },
    /// The named paste buffer, or the most recent one.
    Get {
        name: Option<String>,
    },
    List,
    Delete {
        name: Option<String>,
    },
    /// Renames a buffer (`set-buffer -n`); `name` None is the newest buffer.
    Rename {
        name: Option<String>,
        to: String,
    },
    /// Pastes a buffer into a pane (`paste-buffer`).
    Paste(PasteBufferOpts),
}
