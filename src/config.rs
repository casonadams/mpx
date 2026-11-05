use serde::{Deserialize, Serialize};
use crate::clients::{Size, WindowSize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

/// tmux's `status-left` default.
pub const DEFAULT_STATUS_LEFT: &str = "[#{session_name}] ";
/// tmux's `status-right` default, with a `[copy]` badge while the pane is in copy mode.
pub const DEFAULT_STATUS_RIGHT: &str =
    "#{?pane_in_mode,[copy] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y";
/// tmux's `window-status-format` and `window-status-current-format` default.
pub const DEFAULT_WINDOW_FORMAT: &str = "#I:#W#{window_flags}";

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
    /// Session options: the default terminal, shell and command, detaching, history and restore.
    #[serde(flatten)]
    pub session: SessionOptions,
    /// Terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52): `on` from the active pane,
    /// `all` from every pane the terminal shows, `off` never.
    pub allow_passthrough: Passthrough,
    /// Window options: how windows are named, monitored, numbered and sized (tmux window options).
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
    #[serde(deserialize_with = "status_lines_in_range")]
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
    /// Terminal options: focus events, extended keys, the outer terminal's titles and bells, and
    /// the visual alternatives to bells. Flattened into `Config`, so each key stays top-level in the
    /// config file.
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

/// Window options: how windows are named, monitored, numbered and sized (tmux window options).
/// Flattened into `Config`, so each key stays top-level in the config file.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowOptions {
    /// Rename windows after their foreground process (tmux `automatic-rename`).
    pub automatic_rename: bool,
    /// Flag windows with output the user has not seen (tmux `monitor-activity`).
    pub monitor_activity: bool,
    /// Flag windows that rang the bell (tmux `monitor-bell`).
    pub monitor_bell: bool,
    /// Seconds without output after which a background window is flagged `~` (tmux `monitor-silence`); 0 is off.
    pub monitor_silence: u64,
    /// Whether closing a window renumbers the windows after it (tmux `renumber-windows`); off keeps gaps.
    pub renumber_windows: bool,
    /// Whether windows start with their panes synchronized, so typed input reaches every pane of the window (tmux `synchronize-panes`); `set-option -w` changes a window afterwards.
    pub synchronize_panes: bool,
    /// Pane number of the first pane of a window (tmux `pane-base-index`), as `#{pane_index}` shows it.
    pub pane_base_index: usize,
    /// Which attached clients the window is sized for (tmux `window-size`).
    pub window_size: WindowSize,
    /// Window size under `window-size manual` (tmux `default-size`).
    pub default_size: Size,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            automatic_rename: true,
            monitor_activity: true,
            monitor_bell: true,
            monitor_silence: 0,
            renumber_windows: true,
            synchronize_panes: false,
            pane_base_index: 0,
            window_size: WindowSize::default(),
            default_size: Size::default(),
        }
    }
}

/// Session options: the default terminal, shell and command, detaching, history and restore.
/// Flattened into `Config`, so each key stays top-level in the config file.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionOptions {
    /// Number of exited, removed nodes whose logs stay readable.
    pub history_limit: usize,
    /// Lines of scrollback each pane keeps for captures (tmux `history-limit`).
    pub scrollback_lines: usize,
    /// TERM exported to panes (tmux `default-terminal`).
    pub default_terminal: String,
    /// Program run by a pane started without a command (tmux `default-shell`); unset uses `$SHELL`.
    pub default_shell: Option<String>,
    /// Command run by a pane started without one, through the default shell (tmux `default-command`).
    pub default_command: Option<String>,
    /// Whether a terminal whose session ends is detached (tmux `detach-on-destroy on`); `false`
    /// switches it to another running session instead.
    pub detach_on_destroy: bool,
    /// After a crash or reboot, recreate the last session layout and resume its processes on start.
    pub restore: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            history_limit: 64,
            scrollback_lines: 100_000,
            default_terminal: "xterm-256color".to_string(),
            default_shell: None,
            default_command: None,
            detach_on_destroy: true,
            restore: true,
        }
    }
}

/// Terminal options: input modes the panes may ask for, focus reports, and the outer terminal's
/// title and bell. Flattened into `Config`, so each key stays top-level in the config file.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerminalOptions {
    /// Modified keys sent to panes as CSI u or modifyOtherKeys sequences (tmux `extended-keys`):
    /// `on` when a pane asks for them, `always` regardless, `off` never.
    pub extended_keys: ExtendedKeys,
    /// Function and cursor keys with modifiers keep their modifiers (tmux `xterm-keys`); `off` sends
    /// them unmodified.
    pub xterm_keys: bool,
    /// Focus in and out reach the panes that asked for focus reports (tmux `focus-events`).
    pub focus_events: bool,
    /// Set the outer terminal's title from `set_titles_string` (tmux `set-titles`).
    pub set_titles: bool,
    /// Outer terminal title as a tmux format (tmux `set-titles-string`).
    pub set_titles_string: String,
    /// Which windows' bells ring the outer terminal (tmux `bell-action`).
    pub bell_action: BellAction,
    /// Show a status-row message instead of ringing the outer terminal (tmux `visual-bell`).
    pub visual_bell: bool,
    /// Show a status-row message when a window with monitored output changes (tmux `visual-activity`).
    pub visual_activity: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            extended_keys: ExtendedKeys::Off,
            xterm_keys: true,
            focus_events: false,
            set_titles: false,
            set_titles_string: "#S:#I:#W - \"#T\"".to_string(),
            bell_action: BellAction::Any,
            visual_bell: false,
            visual_activity: false,
        }
    }
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
    /// Style over `status_bg` and `status_fg` for the status line (tmux `status-style`).
    pub status_style: String,
    /// Style of a message on the status row (tmux `message-style`).
    pub message_style: String,
    /// Style of the command prompt on the status row (tmux `message-command-style`).
    pub message_command_style: String,
    /// Style over `border_inactive` for the borders of inactive panes (tmux `pane-border-style`).
    pub pane_border_style: String,
    /// Style over `border_active` for the borders of the active pane (tmux `pane-active-border-style`).
    pub pane_active_border_style: String,
    /// Style of every window tab (tmux `window-status-style`).
    pub window_status_style: String,
    /// Style of the current window's tab (tmux `window-status-current-style`).
    pub window_status_current_style: String,
    /// Style of tabs with output (tmux `window-status-activity-style`).
    pub window_status_activity_style: String,
    /// Style of tabs that rang the bell (tmux `window-status-bell-style`).
    pub window_status_bell_style: String,
    /// Style of the last window's tab (tmux `window-status-last-style`).
    pub window_status_last_style: String,
    /// Dim the colors of inactive panes when multiple are visible.
    pub dim_inactive: bool,
    /// How far inactive panes are dimmed (0.0 = untouched, 1.0 = black).
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    #[default]
    Single,
    Rounded,
    Double,
    Ascii,
}

impl StyleConfig {
    /// Every style string must read as a tmux style, so a bad value is an error at load.
    pub fn check(&self) -> Result<(), String> {
        let styles = [
            ("status_style", &self.status_style),
            ("message_style", &self.message_style),
            ("message_command_style", &self.message_command_style),
            ("pane_border_style", &self.pane_border_style),
            ("pane_active_border_style", &self.pane_active_border_style),
            ("window_status_style", &self.window_status_style),
            (
                "window_status_current_style",
                &self.window_status_current_style,
            ),
            (
                "window_status_activity_style",
                &self.window_status_activity_style,
            ),
            ("window_status_bell_style", &self.window_status_bell_style),
            ("window_status_last_style", &self.window_status_last_style),
        ];
        for (key, value) in styles {
            crate::tui::style::parse(value).map_err(|e| format!("style.{key}: {e}"))?;
        }
        Ok(())
    }
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
            status_style: String::new(),
            message_style: String::new(),
            message_command_style: String::new(),
            pane_border_style: String::new(),
            pane_active_border_style: String::new(),
            window_status_style: String::new(),
            window_status_current_style: String::new(),
            window_status_activity_style: String::new(),
            window_status_bell_style: String::new(),
            window_status_last_style: String::new(),
            dim_inactive: false,
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

/// Declares a word-valued enum: `FromStr` accepts any of a variant's words (after trimming), and
/// `as_str` gives its first word.
macro_rules! word_choice {
    ($ty:ident { $($variant:ident => [$($word:literal),+]),+ $(,)? }) => {
        impl FromStr for $ty {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, String> {
                choice(value, &[$((&[$($word),+], Self::$variant)),+])
            }
        }

        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => [$($word),+][0],)+
                }
            }
        }
    };
}

/// Row of the screen the status line occupies (tmux `status-position`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StatusPosition {
    Top,
    #[default]
    Bottom,
}

word_choice! {
    StatusPosition {
        Top => ["top"],
        Bottom => ["bottom"],
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

word_choice! {
    Justify {
        Left => ["left"],
        Centre => ["centre", "center"],
        Right => ["right"],
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

word_choice! {
    ModeKeys {
        Vi => ["vi"],
        Emacs => ["emacs"],
    }
}

/// Modified keys sent to panes (tmux `extended-keys`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExtendedKeys {
    #[default]
    Off,
    On,
    Always,
}

word_choice! {
    ExtendedKeys {
        Off => ["off"],
        On => ["on"],
        Always => ["always"],
    }
}

/// Which windows' bells reach the outer terminal (tmux `bell-action`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BellAction {
    /// No bell reaches the outer terminal.
    None,
    /// Every window's bell reaches it.
    #[default]
    Any,
    /// Only the bell of the window that is shown.
    Current,
    /// Only the bells of windows that are not shown.
    Other,
}

word_choice! {
    BellAction {
        None => ["none"],
        Any => ["any"],
        Current => ["current"],
        Other => ["other"],
    }
}

impl BellAction {
    /// Whether a bell in a window reaches the outer terminal; `shown` is whether that window is the
    /// one on screen.
    pub fn rings(self, shown: bool) -> bool {
        match self {
            Self::None => false,
            Self::Any => true,
            Self::Current => shown,
            Self::Other => !shown,
        }
    }
}

pub fn default_resume_rules() -> Vec<ResumeRule> {
    vec![
        ResumeRule {
            env_id: Some("CLAUDE_SESSION_ID".into()),
            id_source: Some(IdSource::Claude),
            ..rule(
                "claude",
                &["claude"],
                &["claude", "--resume", "{id}"],
                &["claude", "--continue"],
            )
        },
        ResumeRule {
            env_id: Some("OMP_SESSION_ID".into()),
            id_source: Some(IdSource::Omp),
            ..rule(
                "omp",
                &["omp"],
                &["omp", "--restore", "{id}"],
                &["omp", "--continue"],
            )
        },
        rule(
            "codex",
            &["codex"],
            &["codex", "resume", "{id}"],
            &["codex", "resume", "--last"],
        ),
        rule(
            "rho",
            &["rho"],
            &["rho", "--resume", "{id}"],
            &["rho", "--resume"],
        ),
        rule(
            "antigravity",
            &["antigravity", "agy"],
            &["agy", "--conversation", "{id}"],
            &["agy", "--continue"],
        ),
        rule(
            "opencode",
            &["opencode"],
            &["opencode", "--session", "{id}"],
            &["opencode"],
        ),
        rule(
            "nvim",
            &["nvim"],
            &[],
            &["nvim", "-c", "silent! source Session.vim"],
        ),
    ]
}

/// Rule with no env var or id source; callers override those via struct update.
fn rule(name: &str, detect: &[&str], resume: &[&str], fallback: &[&str]) -> ResumeRule {
    ResumeRule {
        name: name.into(),
        detect: words(detect),
        resume: words(resume),
        fallback: words(fallback),
        env_id: None,
        id_source: None,
    }
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).into()).collect()
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
            status_left: DEFAULT_STATUS_LEFT.to_string(),
            status_right: DEFAULT_STATUS_RIGHT.to_string(),
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

/// Most status rows tmux allows (`status 5`).
pub const MAX_STATUS_LINES: usize = 5;

fn status_lines_in_range<'de, D: serde::Deserializer<'de>>(de: D) -> Result<usize, D::Error> {
    let lines = usize::deserialize(de)?;
    if (1..=MAX_STATUS_LINES).contains(&lines) {
        Ok(lines)
    } else {
        Err(serde::de::Error::custom(format!(
            "status_lines must be 1 to {MAX_STATUS_LINES}, got {lines}"
        )))
    }
}

/// A tmux `status` value: `off` gives `None`; `on` gives one line, and a count 2 to 5 gives that
/// many status lines.
pub fn parse_status(value: &str) -> Result<Option<usize>, String> {
    let text = value.trim();
    if let Ok(lines) = text.parse::<usize>()
        && (2..=MAX_STATUS_LINES).contains(&lines)
    {
        return Ok(Some(lines));
    }
    match parse_switch(text) {
        Ok(on) => Ok(on.then_some(1)),
        Err(_) => Err(format!(
            "expected on, off, or 2 to {MAX_STATUS_LINES} lines, got '{value}'"
        )),
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

/// The choice whose words include `raw` once surrounding whitespace is trimmed.
fn choice<T: Copy>(raw: &str, choices: &[(&[&str], T)]) -> Result<T, String> {
    let value = raw.trim();
    choices
        .iter()
        .find(|(words, _)| words.contains(&value))
        .map(|(_, choice)| *choice)
        .ok_or_else(|| invalid_value(raw))
}

fn switch(value: &str) -> Result<bool, String> {
    parse_switch(value).map_err(|_| invalid_value(value))
}
