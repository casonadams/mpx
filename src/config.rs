use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Prefix key in tmux notation (`C-b`, `C-a`, `C-Space`).
    pub prefix: String,
    /// Keep panes on screen after their process exits: `on`, `off`, or `failed` (only panes that
    /// exited without status 0). A file may also say `true` or `false`.
    pub remain_on_exit: RemainOnExit,
    /// Number of exited, removed nodes whose logs stay readable.
    pub history_limit: usize,
    /// Lines of scrollback each pane keeps for captures (tmux `history-limit`).
    pub scrollback_lines: usize,
    /// Export TMUX/TMUX_PANE and the mpx `tmux` shim in every pane (default: true).
    pub tmux_compat: bool,
    /// Terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52): `on` from the active pane,
    /// `all` from every pane the terminal shows, `off` never.
    pub allow_passthrough: Passthrough,
    /// Rename windows after their foreground process (tmux `automatic-rename`).
    pub automatic_rename: bool,
    /// Flag windows with output the user has not seen (tmux `monitor-activity`).
    pub monitor_activity: bool,
    /// Flag windows that rang the bell (tmux `monitor-bell`).
    pub monitor_bell: bool,
    /// TERM exported to panes (tmux `default-terminal`).
    pub default_terminal: String,
    /// Program run by a pane started without a command (tmux `default-shell`); unset uses `$SHELL`.
    pub default_shell: Option<String>,
    /// Command run by a pane started without one, through the default shell (tmux `default-command`).
    pub default_command: Option<String>,
    /// After a crash or reboot, recreate the last session layout and resume its processes on start.
    pub restore: bool,
    /// Milliseconds after the prefix key before the shortcut popup appears; 0 shows it at once.
    pub which_key_delay_ms: u64,
    /// Milliseconds the display-panes numbers stay up (tmux `display-panes-time`); 0 waits for a key.
    pub display_panes_time_ms: u64,
    /// Window number of the first window (tmux `base-index`). The daemon numbers windows from 0; the
    /// shim and the TUI translate.
    pub base_index: usize,
    /// Pane number of the first pane of a window (tmux `pane-base-index`), as `#{pane_index}` shows it.
    pub pane_base_index: usize,
    /// Mouse input (tmux `mouse`). Off: the terminal's mouse reporting is turned off and mpx ignores mouse events.
    pub mouse: bool,
    /// Whether the status line is shown (tmux `status`). Off: the status row belongs to the panes.
    pub status: bool,
    /// Row the status line occupies (tmux `status-position`).
    pub status_position: StatusPosition,
    /// Seconds between timed status redraws (tmux `status-interval`); 0 redraws only on events.
    pub status_interval: u64,
    /// Left side as a tmux format (tmux `status-left`); `None` keeps mpx's `[session]` label.
    pub status_left: Option<String>,
    /// Right side as a tmux format, strftime applies (tmux `status-right`); `None` keeps mpx's badges and clock.
    pub status_right: Option<String>,
    /// Width limit for a custom `status_left` (tmux `status-left-length`).
    pub status_left_length: usize,
    /// Width limit for a custom `status_right` (tmux `status-right-length`).
    pub status_right_length: usize,
    /// Window tab as a tmux format (tmux `window-status-format`); `None` keeps mpx's built-in tabs.
    pub window_status_format: Option<String>,
    /// Format of the active window tab (tmux `window-status-current-format`); `None` uses `window_status_format`.
    pub window_status_current_format: Option<String>,
    /// Text between window tabs (tmux `window-status-separator`); `None` is a single space.
    pub window_status_separator: Option<String>,
    /// Placement of the window list between the left and right sides (tmux `status-justify`).
    pub status_justify: Justify,
    /// Milliseconds a status-row message stays up (tmux `display-time`).
    pub display_time_ms: u64,
    /// Copy-mode key table (tmux `mode-keys`).
    pub mode_keys: ModeKeys,
    /// Whether copy-mode yanks reach the system clipboard (tmux `set-clipboard`; `off` disables it).
    pub set_clipboard: bool,
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
    /// Desaturate the colors of inactive panes when multiple are visible.
    pub dim_inactive: bool,
    /// How far inactive panes are desaturated (0.0 = untouched, 1.0 = grey).
    pub dim_factor: f32,
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

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct InactiveStyle {
    /// Default foreground text color for inactive panes (e.g. "#4c566a", "bright-black").
    pub fg: String,
    /// Default background color for inactive panes (e.g. "#242933", "black").
    pub bg: String,
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
            dim_factor: 0.5,
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
    /// Optional environment variable that carries the session id (read from the daemon's environment).
    pub env_id: Option<String>,
    /// Where to find the session id of the tool running in the pane, tried before `env_id`.
    pub id_source: Option<IdSource>,
}

/// State files that map a live process to its session.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdSource {
    /// Claude Code: `~/.claude/sessions/<pid>.json`.
    Claude,
    /// oh-my-pi: `~/.omp/agent/terminal-sessions/<tty>`.
    Omp,
}

/// Row of the screen the status line occupies (tmux `status-position`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StatusPosition {
    Top,
    #[default]
    Bottom,
}

impl FromStr for StatusPosition {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value.trim() {
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            _ => Err(invalid_value(value)),
        }
    }
}

/// Horizontal placement of the window list on the status line (tmux `status-justify`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Justify {
    #[default]
    Left,
    Centre,
    Right,
}

impl FromStr for Justify {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value.trim() {
            "left" => Ok(Self::Left),
            "centre" | "center" => Ok(Self::Centre),
            "right" => Ok(Self::Right),
            _ => Err(invalid_value(value)),
        }
    }
}

/// Copy-mode key table (tmux `mode-keys`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModeKeys {
    /// mpx's own vi-style keys.
    #[default]
    Vi,
    Emacs,
}

impl FromStr for ModeKeys {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value.trim() {
            "vi" => Ok(Self::Vi),
            "emacs" => Ok(Self::Emacs),
            _ => Err(invalid_value(value)),
        }
    }
}

pub fn default_resume_rules() -> Vec<ResumeRule> {
    vec![
        ResumeRule {
            name: "claude".into(),
            detect: vec!["claude".into()],
            resume: vec!["claude".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["claude".into(), "--continue".into()],
            env_id: Some("CLAUDE_SESSION_ID".into()),
            id_source: Some(IdSource::Claude),
        },
        ResumeRule {
            name: "omp".into(),
            detect: vec!["omp".into()],
            resume: vec!["omp".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["omp".into(), "--continue".into()],
            env_id: Some("OMP_SESSION_ID".into()),
            id_source: Some(IdSource::Omp),
        },
        ResumeRule {
            name: "codex".into(),
            detect: vec!["codex".into()],
            resume: vec!["codex".into(), "resume".into(), "{id}".into()],
            fallback: vec!["codex".into(), "resume".into(), "--last".into()],
            env_id: None,
            id_source: None,
        },
        ResumeRule {
            name: "rho".into(),
            detect: vec!["rho".into()],
            resume: vec!["rho".into(), "--resume".into(), "{id}".into()],
            fallback: vec!["rho".into(), "--resume".into()],
            env_id: None,
            id_source: None,
        },
        ResumeRule {
            name: "antigravity".into(),
            detect: vec!["antigravity".into(), "agy".into()],
            resume: vec!["agy".into(), "--conversation".into(), "{id}".into()],
            fallback: vec!["agy".into(), "--continue".into()],
            env_id: None,
            id_source: None,
        },
        ResumeRule {
            name: "opencode".into(),
            detect: vec!["opencode".into()],
            resume: vec!["opencode".into(), "--session".into(), "{id}".into()],
            fallback: vec!["opencode".into()],
            env_id: None,
            id_source: None,
        },
        ResumeRule {
            name: "nvim".into(),
            detect: vec!["nvim".into()],
            resume: Vec::new(),
            fallback: vec!["nvim".into(), "-S".into(), "Session.vim".into()],
            env_id: None,
            id_source: None,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "C-b".to_string(),
            remain_on_exit: RemainOnExit::Off,
            history_limit: 64,
            scrollback_lines: 100_000,
            tmux_compat: true,
            allow_passthrough: Passthrough::On,
            automatic_rename: true,
            monitor_activity: true,
            monitor_bell: true,
            default_terminal: "xterm-256color".to_string(),
            default_shell: None,
            default_command: None,
            restore: true,
            which_key_delay_ms: 300,
            display_panes_time_ms: 1000,
            base_index: 0,
            pane_base_index: 0,
            mouse: true,
            status: true,
            status_position: StatusPosition::Bottom,
            status_interval: 1,
            status_left: None,
            status_right: None,
            status_left_length: 10,
            status_right_length: 40,
            window_status_format: None,
            window_status_current_format: None,
            window_status_separator: None,
            status_justify: Justify::Left,
            display_time_ms: 3000,
            mode_keys: ModeKeys::Vi,
            set_clipboard: true,
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

    /// Applies one tmux option (`set-option name value`) that mpx has an equivalent for.
    /// `Ok(false)` for an option with none, which is then only stored.
    pub fn apply_option(&mut self, name: &str, value: &str) -> Result<bool, String> {
        for apply in [Self::apply_session_option, Self::apply_ui_option] {
            if apply(self, name, value)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Options the daemon acts on: panes, windows, output tracking.
    fn apply_session_option(&mut self, name: &str, value: &str) -> Result<bool, String> {
        match name {
            "remain-on-exit" => self.remain_on_exit = value.parse()?,
            "history-limit" => self.scrollback_lines = number(value)?,
            "allow-passthrough" => self.allow_passthrough = value.parse()?,
            "automatic-rename" => self.automatic_rename = switch(value)?,
            "monitor-activity" => self.monitor_activity = switch(value)?,
            "monitor-bell" => self.monitor_bell = switch(value)?,
            "monitor-silence" => {
                if number::<usize>(value)? != 0 {
                    return Err("monitor-silence is not supported by mpx".to_string());
                }
            }
            "display-panes-time" => self.display_panes_time_ms = number(value)?,
            "default-terminal" => {
                if value.trim().is_empty() {
                    return Err(invalid_value(value));
                }
                self.default_terminal = value.to_string();
            }
            "default-shell" => {
                if !value.starts_with('/') {
                    return Err(invalid_value(value));
                }
                self.default_shell = Some(value.to_string());
            }
            "default-command" => {
                self.default_command = Some(value.to_string()).filter(|c| !c.trim().is_empty());
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Options the terminal UI acts on: keys, status line, mouse, numbering.
    fn apply_ui_option(&mut self, name: &str, value: &str) -> Result<bool, String> {
        match name {
            "prefix" => self.prefix = value.to_string(),
            "base-index" => self.base_index = number(value)?,
            "pane-base-index" => self.pane_base_index = number(value)?,
            "renumber-windows" => {
                if !switch(value)? {
                    return Err(RENUMBER_OFF.to_string());
                }
            }
            "mouse" => self.mouse = switch(value)?,
            "status" => self.status = status_switch(value)?,
            "status-position" => self.status_position = value.parse()?,
            "status-interval" => self.status_interval = number(value)?,
            "status-left" => self.status_left = Some(value.to_string()),
            "status-right" => self.status_right = Some(value.to_string()),
            "status-left-length" => self.status_left_length = number(value)?,
            "status-right-length" => self.status_right_length = number(value)?,
            "window-status-format" => self.window_status_format = Some(value.to_string()),
            "window-status-current-format" => {
                self.window_status_current_format = Some(value.to_string());
            }
            "window-status-separator" => self.window_status_separator = Some(value.to_string()),
            "status-justify" => self.status_justify = value.parse()?,
            "status-style" => {
                for (key, color) in style_colors(name, value)? {
                    match key {
                        "fg" => self.style.status_fg = color,
                        _ => self.style.status_bg = color,
                    }
                }
            }
            "status-bg" => self.style.status_bg = colour(name, value)?,
            "status-fg" => self.style.status_fg = colour(name, value)?,
            "pane-border-style" => self.style.border_inactive = border_colour(name, value)?,
            "pane-active-border-style" => self.style.border_active = border_colour(name, value)?,
            "message-style" => {
                return Err(
                    "message-style is not supported: status messages use status-style".to_string(),
                );
            }
            "mode-keys" => self.mode_keys = value.parse()?,
            "display-time" => self.display_time_ms = number(value)?,
            "set-clipboard" => self.set_clipboard = clipboard_switch(value)?,
            _ => return Ok(false),
        }
        Ok(true)
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

    /// Config from `path`, or the defaults when there is none. A missing file yields defaults; a
    /// malformed file is an error.
    pub fn load_at(path: Option<&std::path::Path>) -> Result<Config, String> {
        path.map_or_else(|| Ok(Config::default()), Self::load_file)
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// A tmux on/off value: `on`, `yes`, `true`, `1` / `off`, `no`, `false`, `0`.
pub fn parse_switch(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        other => Err(format!("expected on or off, got '{other}'")),
    }
}

/// A TOML value that is a switch (`true` / `false`, the form older files use) or a word.
#[derive(Deserialize)]
#[serde(untagged)]
enum SwitchOrWord {
    Switch(bool),
    Word(String),
}

/// What happens to a pane whose process ended (tmux `remain-on-exit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(try_from = "SwitchOrWord")]
pub enum RemainOnExit {
    #[default]
    Off,
    On,
    /// Keep only panes that exited without status 0; a signal counts as a failure.
    Failed,
}

impl RemainOnExit {
    /// Whether a pane whose process ended with `code` stays on screen.
    pub fn keeps(self, code: Option<i32>) -> bool {
        match self {
            Self::Off => false,
            Self::On => true,
            Self::Failed => code != Some(0),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Failed => "failed",
        }
    }
}

impl From<bool> for RemainOnExit {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

impl FromStr for RemainOnExit {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        if value.trim().eq_ignore_ascii_case("failed") {
            return Ok(Self::Failed);
        }
        Ok(switch(value)?.into())
    }
}

impl TryFrom<SwitchOrWord> for RemainOnExit {
    type Error = String;

    fn try_from(raw: SwitchOrWord) -> Result<Self, String> {
        match raw {
            SwitchOrWord::Switch(on) => Ok(on.into()),
            SwitchOrWord::Word(word) => word.parse(),
        }
    }
}

/// Terminal passthrough (tmux `allow-passthrough`): from nothing, from the active pane, or from
/// every pane the terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(try_from = "SwitchOrWord")]
pub enum Passthrough {
    Off,
    #[default]
    On,
    All,
}

impl Passthrough {
    /// Whether passthrough sequences from a pane are forwarded; `active` is whether it is the
    /// focused pane.
    pub fn allows(self, active: bool) -> bool {
        match self {
            Self::Off => false,
            Self::On => active,
            Self::All => true,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::All => "all",
        }
    }
}

impl From<bool> for Passthrough {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

impl FromStr for Passthrough {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        if value.trim().eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        Ok(switch(value)?.into())
    }
}

impl TryFrom<SwitchOrWord> for Passthrough {
    type Error = String;

    fn try_from(raw: SwitchOrWord) -> Result<Self, String> {
        match raw {
            SwitchOrWord::Switch(on) => Ok(on.into()),
            SwitchOrWord::Word(word) => word.parse(),
        }
    }
}

fn invalid_value(value: &str) -> String {
    format!("invalid value: {value}")
}

fn switch(value: &str) -> Result<bool, String> {
    parse_switch(value).map_err(|_| invalid_value(value))
}

fn number<T: FromStr>(value: &str) -> Result<T, String> {
    value.trim().parse().map_err(|_| invalid_value(value))
}

/// The refusal for `renumber-windows off`: mpx always numbers windows from 0 with no gaps.
const RENUMBER_OFF: &str = "renumber-windows off is not supported: windows are always renumbered";

/// `status` is `on` or `off`; tmux's multi-line counts (2 to 5) have no equivalent.
fn status_switch(value: &str) -> Result<bool, String> {
    match value.trim() {
        "2" | "3" | "4" | "5" => {
            Err("status: multi-line status bars are not supported".to_string())
        }
        _ => switch(value),
    }
}

/// `set-clipboard` is `on`, `off` or `external` (the same as `on` for mpx).
fn clipboard_switch(value: &str) -> Result<bool, String> {
    match value.trim() {
        "external" => Ok(true),
        _ => switch(value),
    }
}

/// Colour names of a tmux style string such as `fg=red,bg=#112233`. Other items are refused.
fn style_colors(name: &str, value: &str) -> Result<Vec<(&'static str, String)>, String> {
    let mut out = Vec::new();
    for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (key, color) = item.split_once('=').unwrap_or_default();
        let key = match key.trim() {
            "fg" => "fg",
            "bg" => "bg",
            _ => {
                return Err(format!(
                    "{name}: '{item}' is not supported (only fg= and bg=)"
                ));
            }
        };
        out.push((key, colour(name, color.trim())?));
    }
    Ok(out)
}

/// A colour name or hex value the renderer knows.
fn colour(name: &str, color: &str) -> Result<String, String> {
    if crate::tui::render::parse_color(color).is_some() {
        Ok(color.to_string())
    } else {
        Err(format!("{name}: '{color}' is not a colour"))
    }
}

/// The foreground colour of a pane border style; border backgrounds have no equivalent.
fn border_colour(name: &str, value: &str) -> Result<String, String> {
    let mut fg = None;
    for (key, color) in style_colors(name, value)? {
        if key != "fg" {
            return Err(format!("{name}: bg= has no equivalent"));
        }
        fg = Some(color);
    }
    fg.ok_or_else(|| invalid_value(value))
}
