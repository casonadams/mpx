use serde::{Deserialize, Deserializer, Serialize};
use std::str::FromStr;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    #[default]
    Single,
    Rounded,
    Double,
    Ascii,
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

/// Most status rows tmux allows (`status 5`).
const MAX_STATUS_LINES: usize = 5;

pub(super) fn status_lines_in_range<'de, D: Deserializer<'de>>(de: D) -> Result<usize, D::Error> {
    let lines = usize::deserialize(de)?;
    if (1..=MAX_STATUS_LINES).contains(&lines) {
        Ok(lines)
    } else {
        Err(serde::de::Error::custom(format!(
            "status_lines must be 1 to {MAX_STATUS_LINES}, got {lines}"
        )))
    }
}

/// A tmux on/off value: `on`, `yes`, `true`, `1` / `off`, `no`, `false`, `0`.
pub fn parse_switch(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        other => Err(format!("expected on or off, got '{other}'")),
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
