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
    /// Export TMUX/TMUX_PANE and the mpx `tmux` shim in every pane (default: true).
    pub tmux_compat: bool,
    /// Allow terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52) from the active pane.
    pub allow_passthrough: bool,
    /// After a crash or reboot, recreate the last session layout and resume its processes on start.
    pub restore: bool,
    /// Milliseconds after the prefix key before the shortcut popup appears; 0 shows it at once.
    pub which_key_delay_ms: u64,
    /// Extra or overriding key bindings: key (tmux notation) -> command line.
    pub bind: BTreeMap<String, String>,
    /// Extra or overriding bindings for keys pressed without the prefix (tmux `bind -n`).
    pub bind_root: BTreeMap<String, String>,
    /// Rules for resuming processes (AI CLI tools, editors, watchers) across restarts.
    pub resume: Vec<ResumeRule>,
    pub style: StyleConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StyleConfig {
    /// Active pane border color name or hex (e.g. "bright-green", "#3fb950").
    pub border_active: String,
    /// Inactive pane border color name or hex (e.g. "bright-black", "#263345").
    pub border_inactive: String,
    /// Marked pane border color name or hex (e.g. "bright-magenta", "#bc8cff").
    pub border_marked: String,
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
    /// Template styling for the active (focused) pane.
    pub active: ActiveStyle,
    /// Template styling for inactive (unfocused) panes.
    pub inactive: InactiveStyle,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ActiveStyle {
    /// Optional default foreground text color for the active pane (e.g. "#d8dee9", "white").
    pub fg: String,
    /// Optional default background color for the active pane (e.g. "#2e3440", "black").
    pub bg: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum InactiveColorMode {
    #[default]
    Dim,
    Desaturate,
    Tint,
    None,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct InactiveStyle {
    /// Default foreground text color for inactive panes (e.g. "#4c566a", "bright-black").
    pub fg: String,
    /// Default background color for inactive panes (e.g. "#242933", "black").
    pub bg: String,
    /// Transformation applied to colored text in inactive panes: "dim", "desaturate", "tint", or "none".
    pub color_mode: InactiveColorMode,
    /// Transformation intensity (0.0 to 1.0). If omitted, falls back to `style.dim_factor`.
    pub factor: Option<f32>,
    /// Whether to also transform explicit application background colors in inactive panes.
    pub dim_content_bg: Option<bool>,
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
            border_marked: "bright-magenta".to_string(),
            border_style: BorderStyle::Single,
            status_bg: "dark-green".to_string(),
            status_fg: "black".to_string(),
            dim_inactive: true,
            dim_factor: 0.20,
            dim_content_bg: true,
            active: ActiveStyle::default(),
            inactive: InactiveStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ResumeRule {
    pub name: String,
    /// Foreground or command process names that identify this tool.
    pub detect: Vec<String>,
    /// Command line to resume a session; `{id}` is replaced by the session id.
    pub resume: Vec<String>,
    /// Command line fallback when no session id is known (e.g. `["claude", "--continue"]`).
    pub fallback: Vec<String>,
    /// Optional environment variable that carries the session id.
    pub env_id: Option<String>,
}

pub fn default_resume_rules() -> Vec<ResumeRule> {
    vec![
        ResumeRule {
            name: "claude".into(),
            detect: vec!["claude".into()],
            resume: vec!["claude".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["claude".into(), "--continue".into()],
            env_id: Some("CLAUDE_SESSION_ID".into()),
        },
        ResumeRule {
            name: "omp".into(),
            detect: vec!["omp".into()],
            resume: vec!["omp".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["omp".into(), "--continue".into()],
            env_id: Some("OMP_SESSION_ID".into()),
        },
        ResumeRule {
            name: "codex".into(),
            detect: vec!["codex".into()],
            resume: vec!["codex".into(), "resume".into(), "{id}".into()],
            fallback: vec!["codex".into(), "resume".into(), "--last".into()],
            env_id: None,
        },
        ResumeRule {
            name: "rho".into(),
            detect: vec!["rho".into()],
            resume: vec!["rho".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["rho".into(), "--resume".into()],
            env_id: None,
        },
        ResumeRule {
            name: "antigravity".into(),
            detect: vec!["antigravity".into(), "agy".into()],
            resume: vec!["agy".into(), "--conversation".into(), "{id}".into()],
            fallback: vec!["agy".into(), "--continue".into()],
            env_id: None,
        },
        ResumeRule {
            name: "opencode".into(),
            detect: vec!["opencode".into()],
            resume: vec!["opencode".into(), "--session".into(), "{id}".into()],
            fallback: vec!["opencode".into()],
            env_id: None,
        },
        ResumeRule {
            name: "nvim".into(),
            detect: vec!["nvim".into()],
            resume: Vec::new(),
            fallback: vec!["nvim".into(), "-S".into(), "Session.vim".into()],
            env_id: None,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "C-b".to_string(),
            remain_on_exit: false,
            history_limit: 64,
            tmux_compat: true,
            allow_passthrough: true,
            restore: true,
            which_key_delay_ms: 300,
            bind: BTreeMap::new(),
            bind_root: BTreeMap::new(),
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
