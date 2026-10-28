use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Prefix key in tmux notation (`C-b`, `C-a`, `C-Space`).
    pub prefix: String,
    /// Keep panes on screen after their process exits.
    pub remain_on_exit: bool,
    /// Number of exited, removed nodes whose logs stay readable.
    pub history_limit: usize,
    /// Export TMUX/TMUX_PANE and the mpx `tmux` shim in every pane.
    pub tmux_compat: bool,
    /// After a crash or reboot, recreate the last session layout and resume its agents on start.
    pub restore: bool,
    /// Keep pipe messages and consumer cursors on disk across daemon restarts.
    pub persist_pipes: bool,
    /// Milliseconds after the prefix key before the shortcut popup appears; 0 shows it at once.
    pub which_key_delay_ms: u64,
    pub agent: AgentConfig,
    /// Extra or overriding key bindings: key (tmux notation) -> command line.
    pub bind: BTreeMap<String, String>,
    /// Extra or overriding agent harness definitions.
    pub harness: Vec<HarnessConfig>,
    pub style: StyleConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StyleConfig {
    /// Active pane border color name or hex (e.g. "bright-green", "#3fb950").
    pub border_active: String,
    /// Inactive pane border color name or hex (e.g. "bright-black", "#263345").
    pub border_inactive: String,
    /// Border style: "single", "rounded", "double", or "ascii".
    pub border_style: BorderStyle,
    /// Status bar background color name or hex.
    pub status_bg: String,
    /// Status bar foreground color name or hex.
    pub status_fg: String,
    /// Dim output of inactive panes when multiple are visible.
    pub dim_inactive: bool,
    /// Dimming factor for inactive panes (0.0 to 1.0; 0.20 = 20% reduction).
    pub dim_factor: f32,
    /// Whether to also dim explicit application background colors in inactive panes.
    pub dim_content_bg: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    #[default]
    Single,
    Rounded,
    Double,
    Ascii,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            border_active: "bright-green".to_string(),
            border_inactive: "bright-black".to_string(),
            border_style: BorderStyle::Single,
            status_bg: "dark-green".to_string(),
            status_fg: "black".to_string(),
            dim_inactive: true,
            dim_factor: 0.20,
            dim_content_bg: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    /// Quiet period after which an agent without hooks is considered idle.
    pub idle_ms: u64,
    /// Emit an OSC 9 desktop notification when an off-screen agent needs input.
    pub notify: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HarnessConfig {
    pub name: String,
    /// Foreground process names that identify this harness.
    pub detect: Vec<String>,
    /// argv for a fresh session.
    pub command: Vec<String>,
    /// argv to resume; `{id}` is replaced by the session id.
    pub resume: Vec<String>,
    /// Extra argv appended to `command` to pre-assign a session id (`{id}` replaced).
    pub session_id_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "C-b".to_string(),
            remain_on_exit: false,
            history_limit: 64,
            tmux_compat: false,
            restore: true,
            persist_pipes: true,
            which_key_delay_ms: 300,
            agent: AgentConfig::default(),
            bind: BTreeMap::new(),
            harness: Vec::new(),
            style: StyleConfig::default(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            idle_ms: 1500,
            notify: true,
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
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Config from the default location (`Config::path`). Missing file yields defaults; a malformed
    /// file is an error.
    pub fn load() -> Result<Config, String> {
        match Self::path() {
            Some(path) => Self::load_file(&path),
            None => Ok(Config::default()),
        }
    }

    /// Config from `path`. Missing file yields defaults; a malformed file is an error.
    pub fn load_file(path: &std::path::Path) -> Result<Config, String> {
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
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
