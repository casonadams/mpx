//! Full-screen popups drawn over the panes: agent manager, session/window/pane tree, key help.
pub mod agents_view;
pub mod help_view;
pub mod nav;
pub mod tree_view;
pub mod widgets;

use crossterm::event::KeyEvent;
use std::io::{self, Write};

use crate::agents::Harness;
use crate::client::Client;
use crate::config::Config;
use crate::layout::Rect;
use crate::tui::command::Command;

pub use agents_view::AgentsView;
pub use help_view::HelpView;
pub use tree_view::{TreePane, TreeSession, TreeView, TreeWindow};

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayAction {
    None,
    Close,
    /// Close the overlay, then run the commands in order.
    Run(Vec<Command>),
}

pub struct OverlayCtx<'a> {
    pub client: &'a mut Client,
    pub session: &'a str,
    pub config: &'a Config,
    pub registry: &'a [Harness],
    /// Working directory of the active pane.
    pub cwd: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HelpEntry {
    pub key: String,
    pub command: String,
}

pub enum Overlay {
    Agents(AgentsView),
    Tree(TreeView),
    Help(HelpView),
}

impl Overlay {
    pub fn agents() -> Self {
        Overlay::Agents(AgentsView::default())
    }

    pub fn tree() -> Self {
        Overlay::Tree(TreeView::default())
    }

    pub fn help(entries: Vec<HelpEntry>) -> Self {
        Overlay::Help(HelpView::new(entries))
    }

    /// Reload data; called on open, once per second while open, and after actions.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        match self {
            Overlay::Agents(view) => view.refresh(ctx).await,
            Overlay::Tree(view) => view.refresh(ctx).await,
            Overlay::Help(_) => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match self {
            Overlay::Agents(view) => view.handle_key(key),
            Overlay::Tree(view) => view.handle_key(key),
            Overlay::Help(view) => view.handle_key(key),
        }
    }

    /// Draw inside `area` (the app passes a centered rect; the overlay draws its own border).
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        match self {
            Overlay::Agents(view) => view.render(out, area),
            Overlay::Tree(view) => view.render(out, area),
            Overlay::Help(view) => view.render(out, area),
        }
    }
}
