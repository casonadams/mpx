//! State that outlives the daemon: session snapshots (restore after a crash or reboot) and
//! durable pipes. Lives under the state directory, not the runtime directory, which is wiped on reboot.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::layout::tree::LayoutNode;
use crate::protocol::SpawnOpts;

mod files;
mod restore;
mod session_id;
mod snapshot;

pub use files::{adopt_legacy_dir, component, state_dir_with};
pub use restore::plan_restore;
pub use session_id::{claude_session_id, lookup as lookup_session_id, omp_session_id};
pub use snapshot::{decode_snapshot, encode_snapshot};

pub const SNAPSHOT_VERSION: u32 = 1;

/// `$MPX_STATE_DIR`, else `$XDG_STATE_HOME/mpx`, else `%LOCALAPPDATA%\mpx\state` (Windows),
/// else `~/.local/state/mpx`.
pub fn state_dir() -> PathBuf {
    files::state_dir_with(|key| std::env::var_os(key), crate::config::home_dir())
}

/// Per-session directory inside `state_dir()` (session name sanitized for the file system).
pub fn session_dir(session: &str) -> PathBuf {
    state_dir().join(files::component(session))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: u32,
    pub session: String,
    pub saved_ms: u64,
    /// True once the daemon stopped through `kill-server`; false while running or after a crash.
    pub clean_shutdown: bool,
    pub active_window: usize,
    #[serde(default)]
    pub last_window: Option<usize>,
    pub windows: Vec<WindowSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub name: String,
    /// The user renamed the window (no automatic naming).
    pub renamed: bool,
    /// Split tree whose leaves are pane names from `panes`.
    pub layout: LayoutNode,
    pub focused: Option<String>,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub pty: bool,
    /// Live working directory when the snapshot was taken.
    pub cwd: Option<String>,
    /// User-supplied environment only (never the MPX_* / TMUX* variables mpx injects).
    pub env: Vec<(String, String)>,
    pub remain_on_exit: Option<bool>,
    pub tmux_compat: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}
/// The session's snapshot file. Writes are atomic (temp file + rename).
pub struct SnapshotStore {
    path: PathBuf,
}

/// What to spawn to recreate a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RestorePlan {
    pub active_window: usize,
    pub last_window: Option<usize>,
    pub windows: Vec<WindowPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowPlan {
    /// Sticky name for renamed windows; `None` keeps automatic naming.
    pub name: Option<String>,
    /// Layout over the pane names in `panes` (panes that are not restored are pruned from it).
    pub layout: LayoutNode,
    pub focused: Option<String>,
    /// One spawn per restored pane, `SpawnOpts.name` equal to its layout leaf.
    pub panes: Vec<SpawnOpts>,
}

