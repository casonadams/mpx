//! `choose-buffer`: the session's paste buffers, newest first, with a preview of the highlighted one.
//! `list-buffers` uses `buffer_summary` for its lines.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::layout::Rect;
use crate::protocol::Buffer;
use crate::tmux_shim::buffer_sample;
use crate::tui::command::Command;
use crate::tui::overlay::nav::{Filter, FilterEvent, ListState, has_ctrl};
use crate::tui::overlay::widgets::{self, Span, Tone, printable};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const HINTS: &str = "j/k move  Enter paste  d delete  / filter  q close";

/// `name: N bytes: "sample"`, the line `list-buffers` shows for a buffer.
pub fn buffer_summary(buffer: &Buffer) -> String {
    format!(
        "{}: {} bytes: \"{}\"",
        buffer.name,
        buffer.data.len(),
        buffer_sample(&buffer.data)
    )
}

#[derive(Default)]
pub struct BufferView {
    buffers: Vec<Buffer>,
    filter: Filter,
    list: ListState,
}

impl BufferView {
    /// Reloads the buffers; a failed call keeps the list shown before.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        if let Ok(buffers) = ctx.client.list_buffers().await {
            self.buffers = buffers;
        }
        let len = self.visible().len();
        self.list.clamp(len);
    }

    /// Buffers that pass the filter, in the daemon's order (newest first).
    pub fn visible(&self) -> Vec<&Buffer> {
        self.buffers
            .iter()
            .filter(|b| self.filter.matches(&[b.name.as_str(), b.data.as_str()]))
            .collect()
    }

    fn highlighted(&self) -> Option<Buffer> {
        self.visible().get(self.list.cursor).map(|b| (*b).clone())
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
            KeyCode::Enter => match self.highlighted() {
                Some(buffer) if !buffer.data.is_empty() => {
                    OverlayAction::Run(vec![Command::Paste {
                        target: None,
                        text: buffer.data,
                        submit: false,
                    }])
                }
                _ => OverlayAction::Close,
            },
            KeyCode::Char('d') => match self.highlighted() {
                Some(buffer) => OverlayAction::DeleteBuffer(buffer.name),
                None => OverlayAction::None,
            },
            _ => OverlayAction::None,
        }
    }

    /// Draws inside `area`: the list on the left, the preview on the right.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let list_w = area.width * 2 / 5;
        let list = Rect::new(area.x, area.y, list_w, area.height);
        let preview = Rect::new(area.x + list_w, area.y, area.width - list_w, area.height);
        self.render_list(out, list)?;
        self.render_preview(out, preview)
    }

    fn render_list<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::draw_panel(out, area, "Buffers", HINTS)?;
        let prompt = self.filter.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        let visible = self.visible();
        if visible.is_empty() {
            return widgets::put_row(out, rows, 0, &[Span::dim("no buffers")], false);
        }
        let items: Vec<Vec<Span>> = visible
            .iter()
            .map(|b| vec![Span::plain(printable(&buffer_summary(b)))])
            .collect();
        widgets::draw_items(out, rows, &items, self.list.cursor, &self.list)
    }

    fn render_preview<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let Some(buffer) = self.highlighted() else {
            return widgets::draw_preview(out, area, None, &[], None);
        };
        let meta = vec![
            Span::bold(buffer.name.clone(), Tone::Plain),
            Span::dim(format!(" · {} bytes", buffer.data.len())),
        ];
        let lines: Vec<Span> = buffer
            .data
            .lines()
            .take(widgets::preview_height(area, true))
            .map(|line| Span::plain(printable(line)))
            .collect();
        widgets::draw_preview(out, area, Some(meta), &lines, Some("empty buffer"))
    }
}
