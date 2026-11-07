//! Key encoding shared by the TUI (crossterm events), `send-keys` (tmux key names) and key binding specs.
mod csi;
mod encode;
mod kitty;
mod mouse;
mod paste;
mod spec;

pub use encode::{encode_key, encode_key_names, focus_event};
pub use mouse::{encode_mouse_event, encode_sgr_mouse, encode_x10_mouse};
pub use paste::{paste_buffer_bytes, wrap_paste};
pub use spec::KeySpec;

use crate::config::{Config, ExtendedKeys};
use serde::{Deserialize, Serialize};

/// Mouse tracking mode requested by an application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseTracking {
    #[default]
    None,
    /// `CSI ? 1000 h`: report clicks only.
    X10,
    /// `CSI ? 1002 h`: report clicks and button-down drags.
    ButtonEvent,
    /// `CSI ? 1003 h`: report all mouse movement.
    AnyEvent,
}

/// Terminal input modes requested by the program running inside a pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TermModes {
    /// DECCKM (`CSI ? 1 h`): arrows/Home/End use SS3 (`ESC O A`).
    pub app_cursor: bool,
    /// `CSI ? 2004 h`.
    pub bracketed_paste: bool,
    /// `CSI ? 1004 h`.
    pub focus_events: bool,
    /// Mouse tracking protocol.
    pub mouse_tracking: MouseTracking,
    /// `CSI ? 1006 h`: SGR extended coordinates format.
    pub mouse_sgr: bool,
    /// xterm modifyOtherKeys level the program set (`CSI > 4 ; n m`). Levels 1 and 2 send modified
    /// keys as `CSI 27 ; m ; code ~`.
    pub modify_other_keys: u8,
    /// Kitty keyboard protocol flags the program set (`CSI > flags u`): 1 disambiguate, 2 report
    /// event types, 4 report alternate keys, 8 report all keys as escape codes, 16 report
    /// associated text.
    pub kitty_keys: u8,
}

/// What the options allow for keys sent to panes: `extended-keys` and `xterm-keys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPolicy {
    pub extended: ExtendedKeys,
    pub xterm_keys: bool,
}

impl KeyPolicy {
    pub fn of(config: &Config) -> Self {
        Self {
            extended: config.terminal.extended_keys,
            xterm_keys: config.terminal.xterm_keys,
        }
    }
}
