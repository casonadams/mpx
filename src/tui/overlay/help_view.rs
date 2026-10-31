//! Key help: the key/command table with `/` filtering.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::layout::Rect;
use crate::tui::overlay::nav::{Filter, ListState, route_list_key};
use crate::tui::overlay::widgets::{self, Chrome, ListPanel, Span};
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
        self.filter.visible(&self.entries, |e| [e.key.as_str(), e.command.as_str()])
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        let len = self.visible().len();
        if route_list_key(&mut self.filter, &mut self.list, key, len) {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            _ => OverlayAction::None,
        }
    }

    /// Draws inside `area` with its border.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::render_list_view(self, out, area)
    }
}

impl widgets::ListView for HelpView {
    fn panel(&self) -> ListPanel<'_> {
        ListPanel {
            chrome: Chrome::Box,
            title: "Help",
            footer: HINTS,
            prompt: self.filter.prompt(),
            empty: None,
        }
    }

    fn rows(&self) -> impl Iterator<Item = Vec<Span>> + '_ {
        self.visible().into_iter().map(|e| {
            vec![
                Span::plain(format!("{:<KEY_COLS$}", e.key)),
                Span::dim(e.command.as_str()),
            ]
        })
    }

    fn cursor(&self) -> &ListState {
        &self.list
    }
}
