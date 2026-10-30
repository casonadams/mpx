//! Full-screen popups drawn over the panes: agent manager, session/window/pane tree, key help.
pub mod buffer_view;
pub mod help_view;
pub mod nav;
pub mod palette_view;
pub mod text_view;
pub mod tree_view;
pub mod widgets;

use crossterm::event::KeyEvent;
use std::io::{self, Write};
use std::path::Path;

use crate::client::Client;
use crate::config::Config;
use crate::layout::Rect;
use crate::tui::command::Command;
use buffer_view::BufferView;
use text_view::TextView;

pub use buffer_view::buffer_summary;
pub use help_view::HelpView;
pub use palette_view::{PaletteItem, PaletteView};
pub use tree_view::{TreePane, TreeSession, TreeView, TreeWindow};
pub use crate::tui::command::PaletteTab;

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayAction {
    None,
    Close,
    /// Close the overlay, then run the commands in order.
    Run(Vec<Command>),
    /// Delete the named paste buffer; the overlay stays open and reloads.
    DeleteBuffer(String),
}

pub struct OverlayCtx<'a> {
    pub client: &'a mut Client,
    pub session: &'a str,
    pub config: &'a Config,
    /// Working directory of the active pane.
    pub cwd: Option<&'a str>,
    /// Directory holding the session sockets, used to discover other sessions.
    pub socket_dir: &'a Path,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HelpEntry {
    pub key: String,
    pub command: String,
}

pub enum Overlay {
    Tree(TreeView),
    Help(HelpView),
    Palette(PaletteView),
    Buffers(BufferView),
    Text(TextView),
}

impl Overlay {
    pub fn tree() -> Self {
        Overlay::Tree(TreeView::default())
    }

    pub fn help(entries: Vec<HelpEntry>) -> Self {
        Overlay::Help(HelpView::new(entries))
    }

    pub fn palette(view: PaletteView) -> Self {
        Overlay::Palette(view)
    }

    /// Read-only text lines in a scrollable box.
    pub fn text(title: impl Into<String>, lines: Vec<String>) -> Self {
        Overlay::Text(TextView::new(title, lines))
    }

    pub fn buffers() -> Self {
        Overlay::Buffers(BufferView::default())
    }

    /// Reload data; called on open, once per second while open, and after actions.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        match self {
            Overlay::Tree(view) => view.refresh(ctx).await,
            Overlay::Help(_) => {}
            Overlay::Palette(view) => view.refresh(ctx).await,
            Overlay::Buffers(view) => view.refresh(ctx).await,
            Overlay::Text(_) => {}
        }
    }

    /// Brings anything that depends on the selection up to date after a key moved it.
    pub async fn follow_selection(&mut self) {
        match self {
            Overlay::Tree(view) => view.follow_selection().await,
            Overlay::Palette(view) => view.follow_selection().await,
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match self {
            Overlay::Tree(view) => view.handle_key(key),
            Overlay::Help(view) => view.handle_key(key),
            Overlay::Palette(view) => view.handle_key(key),
            Overlay::Buffers(view) => view.handle_key(key),
            Overlay::Text(view) => view.handle_key(key),
        }
    }

    /// Draw inside `area` (the app passes a centered rect; the overlay draws its own border).
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        match self {
            Overlay::Tree(view) => view.render(out, area),
            Overlay::Help(view) => view.render(out, area),
            Overlay::Palette(view) => view.render(out, area),
            Overlay::Buffers(view) => view.render(out, area),
            Overlay::Text(view) => view.render(out, area),
        }
    }
}
