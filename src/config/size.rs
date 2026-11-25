use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// Largest width or height a window or saved screen may have, in cells.
pub const MAX_WINDOW_SIZE: u16 = 10_000;

/// `window-size`: which attached clients the window is sized for (tmux `window-size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowSize {
    /// The most recently active client's size (tmux's default).
    #[default]
    Latest,
    /// The smallest width and height among the attached clients, so every client sees all of it.
    Smallest,
    /// The largest width and height among the attached clients.
    Largest,
    /// `default-size`, whatever the clients' sizes.
    Manual,
}

impl WindowSize {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowSize::Latest => "latest",
            WindowSize::Smallest => "smallest",
            WindowSize::Largest => "largest",
            WindowSize::Manual => "manual",
        }
    }
}

impl FromStr for WindowSize {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value.trim() {
            "latest" => Ok(WindowSize::Latest),
            "smallest" => Ok(WindowSize::Smallest),
            "largest" => Ok(WindowSize::Largest),
            "manual" => Ok(WindowSize::Manual),
            other => Err(format!(
                "not a window size policy: {other} (latest, smallest, largest, manual)"
            )),
        }
    }
}

/// A size in cells, written `WIDTHxHEIGHT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
        }
    }
}

impl FromStr for Size {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        let bad = || format!("not a size: {value} (expected WIDTHxHEIGHT, e.g. 80x24)");
        let (w, h) = value.trim().split_once(['x', 'X']).ok_or_else(bad)?;
        let width: u16 = w.parse().map_err(|_| bad())?;
        let height: u16 = h.parse().map_err(|_| bad())?;
        if width == 0 || height == 0 {
            return Err(bad());
        }
        Ok(Self { width, height })
    }
}

impl TryFrom<String> for Size {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        value.parse()
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}
