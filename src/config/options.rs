use super::values::{BellAction, BorderStyle, ExtendedKeys};
use crate::clients::{Size, WindowSize};
use serde::Deserialize;

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
    pub active: PaneStyle,
    /// Template styling for inactive (unfocused) panes.
    pub inactive: PaneStyle,
}

/// Foreground and background colors of one kind of pane.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PaneStyle {
    /// Foreground text color (e.g. "#d8dee9", "white").
    pub fg: String,
    /// Background color (e.g. "#2e3440", "black").
    pub bg: String,
}

impl StyleConfig {
    /// Every style string must read as a tmux style, so a bad value is an error at load.
    pub fn check(&self) -> Result<(), String> {
        let check = |key: &str, value: &str| {
            crate::tui::style::parse(value).map_err(|e| format!("style.{key}: {e}"))
        };
        check("status_style", &self.status_style)?;
        check("message_style", &self.message_style)?;
        check("message_command_style", &self.message_command_style)?;
        check("pane_border_style", &self.pane_border_style)?;
        check("pane_active_border_style", &self.pane_active_border_style)?;
        check("window_status_style", &self.window_status_style)?;
        check(
            "window_status_current_style",
            &self.window_status_current_style,
        )?;
        check(
            "window_status_activity_style",
            &self.window_status_activity_style,
        )?;
        check("window_status_bell_style", &self.window_status_bell_style)?;
        check("window_status_last_style", &self.window_status_last_style)?;
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
            active: PaneStyle::default(),
            inactive: PaneStyle::default(),
        }
    }
}
