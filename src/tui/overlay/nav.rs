//! Vim-style list motion and the `/` filter line shared by the overlay views.
use std::cell::Cell;
use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const DEFAULT_HEIGHT: usize = 20;

pub fn has_ctrl(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Key routing shared by the filterable overlay lists: the `/` filter line first, then list
/// motion over `len` visible rows. Returns true when the key was consumed; false leaves it to
/// the view's own commands.
pub fn route_list_key(
    filter: &mut Filter,
    list: &mut ListState,
    key: KeyEvent,
    len: usize,
) -> bool {
    match filter.handle(key) {
        FilterEvent::Edited => {
            list.cursor = 0;
            return true;
        }
        FilterEvent::Consumed => return true,
        FilterEvent::Ignored => {}
    }
    list.motion(key, len) || has_ctrl(key)
}

/// Case-insensitive subsequence match; an empty query matches everything.
pub fn fuzzy_match(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    query.to_lowercase().chars().all(|q| chars.any(|c| c == q))
}

#[derive(Debug, Clone, Default)]
pub struct ListState {
    pub cursor: usize,
    pending_g: bool,
    top: Cell<usize>,
    height: Cell<usize>,
}

impl ListState {
    /// Applies a motion to a list of `len` rows. Returns false when the key is not a motion.
    pub fn motion(&mut self, key: KeyEvent, len: usize) -> bool {
        let pending_g = std::mem::take(&mut self.pending_g);
        let last = len.saturating_sub(1);
        let rendered = if self.height.get() == 0 {
            DEFAULT_HEIGHT
        } else {
            self.height.get()
        };
        let half = (rendered / 2).max(1);
        let next = match key.code {
            KeyCode::Char('g') if pending_g && !has_ctrl(key) => 0,
            KeyCode::Char('g') if !has_ctrl(key) => {
                self.pending_g = true;
                return true;
            }
            KeyCode::Char('G') => last,
            KeyCode::Char('j') if !has_ctrl(key) => self.cursor + 1,
            KeyCode::Down => self.cursor + 1,
            KeyCode::Char('k') if !has_ctrl(key) => self.cursor.saturating_sub(1),
            KeyCode::Up => self.cursor.saturating_sub(1),
            KeyCode::Char('d') if has_ctrl(key) => self.cursor + half,
            KeyCode::Char('u') if has_ctrl(key) => self.cursor.saturating_sub(half),
            _ => return false,
        };
        self.cursor = next.min(last);
        true
    }

    pub fn clamp(&mut self, len: usize) {
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }

    /// Visible index range for `len` rows shown in `height` lines, keeping `pos` on screen.
    pub fn window(&self, pos: usize, len: usize, height: usize) -> Range<usize> {
        self.height.set(height);
        let span = height.max(1);
        let mut top = self.top.get();
        if pos < top {
            top = pos;
        } else if pos >= top + span {
            top = pos + 1 - span;
        }
        top = top.min(len.saturating_sub(span));
        self.top.set(top);
        top..(top + height).min(len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterEvent {
    Ignored,
    Consumed,
    Edited,
}

/// `/` filter line: typing narrows the list; Enter keeps the text, Esc clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub text: String,
    pub editing: bool,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        self.editing || !self.text.is_empty()
    }

    pub fn matches(&self, fields: &[&str]) -> bool {
        self.text.is_empty() || fields.iter().any(|f| fuzzy_match(&self.text, f))
    }

    /// The `items` whose `fields` pass the filter, in their original order.
    pub fn visible<'a, T>(
        &self,
        items: &'a [T],
        fields: impl Fn(&T) -> [&str; 2],
    ) -> Vec<&'a T> {
        items.iter().filter(|&item| self.matches(&fields(item))).collect()
    }

    pub fn prompt(&self) -> Option<String> {
        self.is_active()
            .then(|| format!("/{}{}", self.text, if self.editing { "_" } else { "" }))
    }

    pub fn handle(&mut self, key: KeyEvent) -> FilterEvent {
        if self.editing {
            self.edit_key(key)
        } else {
            self.start_key(key)
        }
    }

    fn start_key(&mut self, key: KeyEvent) -> FilterEvent {
        match key.code {
            KeyCode::Char('/') if !has_ctrl(key) => {
                self.editing = true;
                FilterEvent::Consumed
            }
            KeyCode::Esc if !self.text.is_empty() => {
                self.text.clear();
                FilterEvent::Edited
            }
            _ => FilterEvent::Ignored,
        }
    }

    fn edit_key(&mut self, key: KeyEvent) -> FilterEvent {
        match key.code {
            KeyCode::Enter => {
                self.editing = false;
                FilterEvent::Consumed
            }
            KeyCode::Esc => {
                self.text.clear();
                self.editing = false;
                FilterEvent::Edited
            }
            KeyCode::Backspace => {
                if self.text.pop().is_none() {
                    self.editing = false;
                }
                FilterEvent::Edited
            }
            KeyCode::Char(c) if !has_ctrl(key) => {
                self.text.push(c);
                FilterEvent::Edited
            }
            _ => FilterEvent::Consumed,
        }
    }
}
