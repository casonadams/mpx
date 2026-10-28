//! Key help: the key/command table with `/` filtering.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::layout::Rect;
use crate::tui::overlay::nav::{Filter, FilterEvent, ListState, has_ctrl};
use crate::tui::overlay::widgets::{self, Span};
use crate::tui::overlay::{HelpEntry, OverlayAction};

const HINTS: &str = "j/k move  gg/G top/bottom  / filter  q close";
const KEY_COLS: usize = 16;

pub struct HelpView {
    entries: Vec<HelpEntry>,
    filter: Filter,
    list: ListState,
}

impl HelpView {
    pub fn new(entries: Vec<HelpEntry>) -> Self {
        Self {
            entries,
            filter: Filter::default(),
            list: ListState::default(),
        }
    }

    /// Entries that pass the current filter, in their original order.
    pub fn visible(&self) -> Vec<&HelpEntry> {
        self.entries
            .iter()
            .filter(|e| self.filter.matches(&[e.key.as_str(), e.command.as_str()]))
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match self.filter.handle(key) {
            FilterEvent::Edited => {
                self.list.cursor = 0;
                return OverlayAction::None;
            }
            FilterEvent::Consumed => return OverlayAction::None,
            FilterEvent::Ignored => {}
        }
        let len = self.visible().len();
        if self.list.motion(key, len) || has_ctrl(key) {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            _ => OverlayAction::None,
        }
    }

    /// Draws inside `area` with its border.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::draw_box(out, area, "Help", HINTS)?;
        let prompt = self.filter.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        let items: Vec<Vec<Span>> = self
            .visible()
            .into_iter()
            .map(|e| {
                vec![
                    Span::plain(format!("{:<KEY_COLS$}", e.key)),
                    Span::dim(e.command.as_str()),
                ]
            })
            .collect();
        widgets::draw_items(out, rows, &items, self.list.cursor, &self.list)
    }
}
