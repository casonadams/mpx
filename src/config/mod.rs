mod options;
mod resume;
mod values;

pub use options::{PaneStyle, SessionOptions, StyleConfig, TerminalOptions, WindowOptions};
pub use resume::{IdSource, ResumeRule, default_resume_rules};
pub use values::{
    BellAction, BorderStyle, ExtendedKeys, Justify, ModeKeys, Passthrough, RemainOnExit,
    StatusPosition, parse_status, parse_switch,
};

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// tmux's `window-status-format` and `window-status-current-format` default.
const DEFAULT_WINDOW_FORMAT: &str = "#I:#W#{window_flags}";

/// A table of command lines keyed by name or key.
pub type CommandTable = BTreeMap<String, String>;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Prefix key in tmux notation (`C-b`, `C-a`, `C-Space`).
    pub prefix: String,
    /// Keep panes on screen after their process exits: `on`, `off`, or `failed` (only panes that
    /// exited without status 0). A file may also say `true` or `false`.
    pub remain_on_exit: RemainOnExit,
    #[serde(flatten)]
    pub session: SessionOptions,
    /// Terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52): `on` from the active pane,
    /// `all` from every pane the terminal shows, `off` never.
    pub allow_passthrough: Passthrough,
    #[serde(flatten)]
    pub window: WindowOptions,
    /// Milliseconds after the prefix key before the shortcut popup appears; 0 shows it at once.
    pub which_key_delay_ms: u64,
    /// Milliseconds the display-panes numbers stay up (tmux `display-panes-time`); 0 waits for a key.
    pub display_panes_time_ms: u64,
    /// Window number of the first window (tmux `base-index`); the daemon numbers from 0, the TUI translates.
    pub base_index: usize,
    /// Mouse input (tmux `mouse`). Off: the terminal's mouse reporting is turned off and mpx ignores mouse events.
    pub mouse: bool,
    /// Whether the status line is shown (tmux `status`). Off: the status row belongs to the panes.
    pub status: bool,
    /// Number of status rows (tmux `status 2..5`), 1 to 5. Row 0 is the status line; rows 1 and up
    /// draw `status_format`.
    #[serde(deserialize_with = "values::status_lines_in_range")]
    pub status_lines: usize,
    /// Formats of the status rows by tmux index: `status-format[i]` draws row `i`. A non-empty
    /// entry 0 replaces the main status line (window list and `status_left`/`status_right` included).
    /// Entries past the row count are ignored.
    pub status_format: Vec<String>,
    /// Row the status line occupies (tmux `status-position`).
    pub status_position: StatusPosition,
    /// Seconds between timed status redraws (tmux `status-interval`); 0 redraws only on events.
    pub status_interval: u64,
    /// Left side as a tmux format (tmux `status-left`).
    pub status_left: String,
    /// Right side as a tmux format, strftime applies (tmux `status-right`).
    pub status_right: String,
    /// Width limit for `status_left` (tmux `status-left-length`).
    pub status_left_length: usize,
    /// Width limit for `status_right` (tmux `status-right-length`).
    pub status_right_length: usize,
    /// Window tab as a tmux format (tmux `window-status-format`).
    pub window_status_format: String,
    /// Format of the active window tab (tmux `window-status-current-format`).
    pub window_status_current_format: String,
    /// Text between window tabs (tmux `window-status-separator`).
    pub window_status_separator: String,
    /// Placement of the window list between the left and right sides (tmux `status-justify`).
    pub status_justify: Justify,
    /// Milliseconds a status-row message stays up (tmux `display-time`).
    pub display_time_ms: u64,
    /// Copy-mode key table (tmux `mode-keys`).
    pub mode_keys: ModeKeys,
    /// Whether copy-mode yanks reach the system clipboard (tmux `set-clipboard`; `off` disables it).
    pub set_clipboard: bool,
    #[serde(flatten)]
    pub terminal: TerminalOptions,
    /// Whether copy-mode searches wrap around the ends of the history (tmux `wrap-search`).
    pub wrap_search: bool,
    /// Extra or overriding key bindings: key (tmux notation) -> command line.
    pub bind: CommandTable,
    /// Extra or overriding bindings for keys pressed without the prefix (tmux `bind -n`).
    pub bind_root: CommandTable,
    /// Command aliases (tmux `command-alias`): a command name -> the command line it stands for.
    pub alias: CommandTable,
    /// Rules for resuming processes (AI CLI tools, editors, watchers) across restarts.
    pub resume: Vec<ResumeRule>,
    pub style: StyleConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "C-b".to_string(),
            remain_on_exit: RemainOnExit::Off,
            session: SessionOptions::default(),
            allow_passthrough: Passthrough::On,
            window: WindowOptions::default(),
            which_key_delay_ms: 300,
            display_panes_time_ms: 1000,
            base_index: 0,
            mouse: true,
            status: true,
            status_lines: 1,
            status_format: Vec::new(),
            status_position: StatusPosition::Bottom,
            status_interval: 1,
            status_left: "[#{session_name}] ".to_string(),
            status_right: "#{?pane_in_mode,[copy] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y"
                .to_string(),
            status_left_length: 10,
            status_right_length: 40,
            window_status_format: DEFAULT_WINDOW_FORMAT.to_string(),
            window_status_current_format: DEFAULT_WINDOW_FORMAT.to_string(),
            window_status_separator: " ".to_string(),
            status_justify: Justify::Left,
            display_time_ms: 3000,
            mode_keys: ModeKeys::Vi,
            set_clipboard: true,
            wrap_search: true,
            terminal: TerminalOptions::default(),
            bind: BTreeMap::new(),
            bind_root: BTreeMap::new(),
            alias: BTreeMap::new(),
            resume: default_resume_rules(),
            style: StyleConfig::default(),
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("MPX_CONFIG") {
            return Some(PathBuf::from(p));
        }
        if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(dir).join("mpx").join("config.toml"));
        }
        #[cfg(windows)]
        if let Ok(dir) = std::env::var("APPDATA") {
            return Some(PathBuf::from(dir).join("mpx").join("config.toml"));
        }
        home_dir().map(|h| h.join(".config").join("mpx").join("config.toml"))
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.style.check()?;
        Ok(config)
    }

    /// Config from the default location (`Config::path`). Missing file yields defaults; a malformed
    /// file is an error.
    pub fn load() -> Result<Config, String> {
        Self::load_at(Self::path().as_deref())
    }

    /// Config from `path`. Missing file yields defaults; a malformed file is an error.
    pub fn load_file(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {}", path.display(), e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {}", path.display(), e)),
        }
    }

    pub fn load_or_default() -> Config {
        Self::load().unwrap_or_else(|e| {
            tracing::warn!("ignoring invalid mpx config: {}", e);
            Config::default()
        })
    }

    /// Config from `path`, or the defaults when there is none. A missing file yields defaults; a
    /// malformed file is an error.
    pub fn load_at(path: Option<&Path>) -> Result<Config, String> {
        path.map_or_else(|| Ok(Config::default()), Self::load_file)
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
