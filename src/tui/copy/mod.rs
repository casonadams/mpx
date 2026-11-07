//! Copy (vi-style visual) mode: cursor, scroll, selection, search and yank.
mod action;
mod apply;
mod motion;
mod search;
mod selection;
mod view;

pub use action::{CopyAction, CopyWhat, parse_copy_command};
pub use apply::{apply_action, apply_bound_key, apply_key};
pub use motion::Motion;
pub use search::{find_next_match, line_matches};
pub use selection::char_selection_text;

use super::prompt::PromptState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionKind {
    #[default]
    None,
    Character,
    Line,
    /// `rectangle-toggle`: a selection covering the rectangle between its corners.
    Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Up,
    Down,
}

impl SearchDirection {
    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    pub fn prompt_char(self) -> char {
        match self {
            Self::Up => '?',
            Self::Down => '/',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPrompt {
    pub direction: SearchDirection,
    pub prompt: PromptState,
}

impl SearchPrompt {
    pub fn new(direction: SearchDirection) -> Self {
        Self {
            direction,
            prompt: PromptState::new(""),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CopyState {
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub scroll_offset: usize,
    pub anchor_x: usize,
    pub anchor_abs_y: usize,
    pub mode: SelectionKind,
    pub search_prompt: Option<SearchPrompt>,
    pub last_search: Option<(SearchDirection, String)>,
    /// Entered by the wheel: scrolling it back down to the live screen leaves copy mode, as
    /// tmux's `copy-mode -e` does. Entered from the keyboard, only `q` or Esc leave.
    pub wheel_exit: bool,
    /// `set-mark`: the row of the marked line (a `Grid::row_at` number), if one is set.
    pub mark: Option<usize>,
    /// Whether searches wrap around the ends of the history (the `wrap-search` option).
    pub wrap_search: bool,
}

/// What a copy-mode command asks the app to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    Stay,
    /// Leaves copy mode.
    Exit,
    /// Copies the text (paste buffer and clipboard), then leaves copy mode.
    Yank(String),
    /// Copies the text and stays in copy mode.
    Copy(String),
    /// Copies the text, pipes it to the shell `command`, and leaves copy mode when `exit`.
    Pipe {
        text: String,
        command: String,
        exit: bool,
    },
    SearchFailed(String),
}

/// Pane size in cells, used to clamp cursor motion.
pub type PaneSize = (u16, u16);

/// Rows moved by one mouse-wheel notch.
pub const WHEEL_STEP: usize = 3;
