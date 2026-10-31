//! `choose-buffer`: the session's paste buffers, newest first, with a preview of the highlighted one.
//! `list-buffers` uses `buffer_summary` for its lines.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::layout::Rect;
use crate::protocol::Buffer;
use crate::tui::command::Command;
use crate::tui::overlay::nav::{Filter, ListState, route_list_key};
use crate::tui::overlay::widgets::{self, Chrome, ListPanel, Span, Tone, printable};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const HINTS: &str = "j/k move  Enter paste  d delete  / filter  q close";
const SAMPLE_CHARS: usize = 50;

/// The first characters of a buffer, control characters shown as escapes.
fn buffer_sample(data: &str) -> String {
    let mut out = String::new();
    for (i, c) in data.chars().enumerate() {
        if i == SAMPLE_CHARS {
            out.push_str("...");
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

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
        self.filter.visible(&self.buffers, |b| [b.name.as_str(), b.data.as_str()])
    }

    fn highlighted(&self) -> Option<Buffer> {
        self.visible().get(self.list.cursor).map(|b| (*b).clone())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        let len = self.visible().len();
        if route_list_key(&mut self.filter, &mut self.list, key, len) {
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
        widgets::render_list_view(self, out, list)?;
        self.render_preview(out, preview)
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

impl widgets::ListView for BufferView {
    fn panel(&self) -> ListPanel<'_> {
        ListPanel {
            chrome: Chrome::Panel,
            title: "Buffers",
            footer: HINTS,
            prompt: self.filter.prompt(),
            empty: Some("no buffers"),
        }
    }

    fn rows(&self) -> impl Iterator<Item = Vec<Span>> + '_ {
        self.visible()
            .into_iter()
            .map(|b| vec![Span::plain(printable(&buffer_summary(b)))])
    }

    fn cursor(&self) -> &ListState {
        &self.list
    }
}
