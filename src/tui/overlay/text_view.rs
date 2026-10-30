//! Read-only text in a scrollable box: `show-messages`, `list-buffers` and multi-line command output.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::layout::Rect;
use crate::tui::overlay::OverlayAction;
use crate::tui::overlay::nav::{ListState, has_ctrl};
use crate::tui::overlay::widgets::{self, Span, printable};

const HINTS: &str = "j/k move  Ctrl-d/Ctrl-u page  gg/G top/bottom  q close";

pub struct TextView {
    title: String,
    lines: Vec<String>,
    list: ListState,
}

impl TextView {
    pub fn new(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines,
            list: ListState::default(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        if self.list.motion(key, self.lines.len()) {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Char('c') if has_ctrl(key) => OverlayAction::Close,
            _ => OverlayAction::None,
        }
    }

    /// Draws inside `area` with its border.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::draw_box(out, area, &self.title, HINTS)?;
        let rows = widgets::body(area, false);
        if self.lines.is_empty() {
            return widgets::put_row(out, rows, 0, &[Span::dim("nothing to show")], false);
        }
        let items: Vec<Vec<Span>> = self
            .lines
            .iter()
            .map(|line| vec![Span::plain(printable(line))])
            .collect();
        widgets::draw_items(out, rows, &items, self.list.cursor, &self.list)
    }
}
