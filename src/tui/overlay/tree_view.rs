//! Choose-tree: sessions, their windows and their panes across every reachable mpx session.
use std::collections::BTreeSet;
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::client::Client;
use crate::ipc::list_sessions;
use crate::layout::{Rect, WindowTab};
use crate::protocol::{AgentState, NodeInfo, PaneId};
use crate::tui::command::Command;
use crate::tui::overlay::nav::{Filter, FilterEvent, ListState, has_ctrl};
use crate::tui::overlay::widgets::{self, Span, Tone, state_mark};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const HINTS: &str = "j/k move  l/Enter open  h collapse  / filter  x kill  q close";
const LAYOUT_WIDTH: u16 = 200;
const LAYOUT_HEIGHT: u16 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSession {
    pub name: String,
    pub windows: Vec<TreeWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeWindow {
    pub index: usize,
    pub name: String,
    pub active: bool,
    pub panes: Vec<TreePane>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreePane {
    pub name: String,
    pub id: PaneId,
    pub agent: Option<AgentState>,
    pub active: bool,
}

/// A visible row: indices into the session, window and pane lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Session(usize),
    Window(usize, usize),
    Pane(usize, usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CollapseKey {
    Session(String),
    Window(String, usize),
}

#[derive(Default)]
pub struct TreeView {
    sessions: Vec<TreeSession>,
    current: String,
    collapsed: BTreeSet<CollapseKey>,
    filter: Filter,
    list: ListState,
}

/// Builds one session's tree from its node list, window tabs and focused pane name.
pub fn session_from_snapshot(
    name: String,
    nodes: &[NodeInfo],
    tabs: &[WindowTab],
    focused: Option<&str>,
) -> TreeSession {
    let windows = tabs
        .iter()
        .map(|tab| TreeWindow {
            index: tab.index,
            name: tab.name.clone(),
            active: tab.is_active,
            panes: nodes
                .iter()
                .filter(|n| n.window == Some(tab.index))
                .map(|n| TreePane {
                    name: n.name.clone(),
                    id: n.id,
                    agent: n.agent.as_ref().map(|a| a.state),
                    active: focused == Some(n.name.as_str()),
                })
                .collect(),
        })
        .collect();
    TreeSession { name, windows }
}

async fn load_session(endpoint: &str) -> Option<TreeSession> {
    let mut client = Client::connect(endpoint).await.ok()?;
    let info = client.info().await.ok()?;
    let nodes = client.list().await.ok()?;
    let (panes, _, tabs) = client.layout(LAYOUT_WIDTH, LAYOUT_HEIGHT).await.ok()?;
    let focused = panes.iter().find(|p| p.2).map(|p| p.0.clone());
    Some(session_from_snapshot(
        info.name,
        &nodes,
        &tabs,
        focused.as_deref(),
    ))
}

impl TreeView {
    pub fn from_data(sessions: Vec<TreeSession>, current: &str) -> Self {
        Self {
            sessions,
            current: current.to_string(),
            ..Self::default()
        }
    }

    /// Reloads every reachable session; sessions that fail to answer are skipped.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        let mut sessions = Vec::new();
        for endpoint in list_sessions() {
            if let Some(session) = load_session(&endpoint.endpoint).await {
                sessions.push(session);
            }
        }
        self.sessions = sessions;
        self.current = ctx.session.to_string();
        let len = self.rows().len();
        self.list.clamp(len);
    }

    /// Labels of the visible rows in display order.
    pub fn labels(&self) -> Vec<String> {
        self.rows()
            .into_iter()
            .map(|n| self.name(n).to_string())
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
        let len = self.rows().len();
        if self.list.motion(key, len) || has_ctrl(key) {
            return OverlayAction::None;
        }
        self.command_for(key.code)
    }

    fn command_for(&mut self, code: KeyCode) -> OverlayAction {
        if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
            return OverlayAction::Close;
        }
        let Some(node) = self.selected() else {
            return OverlayAction::None;
        };
        match code {
            KeyCode::Char('h') | KeyCode::Left => self.collapse(node),
            KeyCode::Char('l') | KeyCode::Right => return self.expand_or_activate(node),
            KeyCode::Enter => return self.activate(node),
            KeyCode::Char('x') => return self.kill(node),
            _ => {}
        }
        OverlayAction::None
    }

    fn selected(&self) -> Option<Node> {
        self.rows().get(self.list.cursor).copied()
    }

    fn name(&self, node: Node) -> &str {
        match node {
            Node::Session(s) => &self.sessions[s].name,
            Node::Window(s, w) => &self.sessions[s].windows[w].name,
            Node::Pane(s, w, p) => &self.sessions[s].windows[w].panes[p].name,
        }
    }

    fn children(&self, node: Node) -> Vec<Node> {
        match node {
            Node::Session(s) => (0..self.sessions[s].windows.len())
                .map(|w| Node::Window(s, w))
                .collect(),
            Node::Window(s, w) => (0..self.sessions[s].windows[w].panes.len())
                .map(|p| Node::Pane(s, w, p))
                .collect(),
            Node::Pane(..) => Vec::new(),
        }
    }

    fn parent(node: Node) -> Option<Node> {
        match node {
            Node::Session(_) => None,
            Node::Window(s, _) => Some(Node::Session(s)),
            Node::Pane(s, w, _) => Some(Node::Window(s, w)),
        }
    }

    fn collapse_key(&self, node: Node) -> Option<CollapseKey> {
        match node {
            Node::Session(s) => Some(CollapseKey::Session(self.sessions[s].name.clone())),
            Node::Window(s, w) => Some(CollapseKey::Window(self.sessions[s].name.clone(), w)),
            Node::Pane(..) => None,
        }
    }

    /// Children are shown unless the node is collapsed; an active filter always expands.
    fn expanded(&self, node: Node) -> bool {
        !self.filter.text.is_empty()
            || !self
                .collapse_key(node)
                .is_some_and(|k| self.collapsed.contains(&k))
    }

    fn has_match(&self, node: Node) -> bool {
        self.filter.matches(&[self.name(node)])
            || self.children(node).into_iter().any(|c| self.has_match(c))
    }

    fn shown(&self, node: Node) -> bool {
        self.filter.text.is_empty() || self.has_match(node)
    }

    fn rows(&self) -> Vec<Node> {
        let mut out = Vec::new();
        for s in 0..self.sessions.len() {
            self.push_visible(Node::Session(s), &mut out);
        }
        out
    }

    fn push_visible(&self, node: Node, out: &mut Vec<Node>) {
        if !self.shown(node) {
            return;
        }
        out.push(node);
        if self.expanded(node) {
            for child in self.children(node) {
                self.push_visible(child, out);
            }
        }
    }

    fn select(&mut self, node: Node) {
        if let Some(pos) = self.rows().iter().position(|n| *n == node) {
            self.list.cursor = pos;
        }
    }

    fn collapse(&mut self, node: Node) {
        if !self.children(node).is_empty() && self.expanded(node) {
            if let Some(key) = self.collapse_key(node) {
                self.collapsed.insert(key);
            }
        } else if let Some(parent) = Self::parent(node) {
            self.select(parent);
        }
    }

    fn expand_or_activate(&mut self, node: Node) -> OverlayAction {
        let collapsed_key = if self.expanded(node) {
            None
        } else {
            self.collapse_key(node)
        };
        match collapsed_key {
            Some(key) => {
                self.collapsed.remove(&key);
                OverlayAction::None
            }
            None => self.activate(node),
        }
    }

    fn pane_target(&self, s: usize, w: usize, p: usize) -> String {
        let pane = &self.sessions[s].windows[w].panes[p];
        format!("{}:%{}", self.sessions[s].name, pane.id)
    }

    /// Commands that make session `s` the current one; empty when it already is.
    fn switch_to(&self, s: usize) -> Vec<Command> {
        let name = &self.sessions[s].name;
        if *name == self.current {
            Vec::new()
        } else {
            vec![Command::SwitchSession { name: name.clone() }]
        }
    }

    fn activate(&self, node: Node) -> OverlayAction {
        let commands = match node {
            Node::Session(s) => self.switch_to(s),
            Node::Window(s, w) => {
                let mut commands = self.switch_to(s);
                commands.push(Command::SelectWindow {
                    index: self.sessions[s].windows[w].index,
                });
                commands
            }
            Node::Pane(s, w, p) => vec![Command::SelectPane {
                target: self.pane_target(s, w, p),
            }],
        };
        if commands.is_empty() {
            OverlayAction::Close
        } else {
            OverlayAction::Run(commands)
        }
    }

    /// Confirms the kill; windows and sessions of other sessions switch there first.
    fn kill(&self, node: Node) -> OverlayAction {
        let (prompt, then, session) = match node {
            Node::Session(s) => (
                format!("kill session {}?", self.sessions[s].name),
                Command::KillServer,
                Some(s),
            ),
            Node::Window(s, w) => {
                let window = &self.sessions[s].windows[w];
                (
                    format!("kill window {}?", window.name),
                    Command::KillWindow {
                        index: Some(window.index),
                    },
                    Some(s),
                )
            }
            Node::Pane(s, w, p) => (
                format!("kill pane {}?", self.sessions[s].windows[w].panes[p].name),
                Command::KillPane {
                    target: Some(self.pane_target(s, w, p)),
                },
                None,
            ),
        };
        let mut commands = session.map_or_else(Vec::new, |s| self.switch_to(s));
        commands.push(Command::Confirm {
            prompt,
            then: Box::new(then),
        });
        OverlayAction::Run(commands)
    }

    fn arrow(&self, node: Node) -> char {
        if self.children(node).is_empty() {
            ' '
        } else if self.expanded(node) {
            '▾'
        } else {
            '▸'
        }
    }

    fn item_spans(&self, node: Node) -> Vec<Span> {
        let star = |active: bool| if active { " *" } else { "" };
        match node {
            Node::Session(s) => {
                let session = &self.sessions[s];
                let tone = if session.name == self.current {
                    Tone::Active
                } else {
                    Tone::Plain
                };
                let current = star(session.name == self.current);
                vec![Span::toned(
                    format!("{} {}{current}", self.arrow(node), session.name),
                    tone,
                )]
            }
            Node::Window(s, w) => {
                let window = &self.sessions[s].windows[w];
                vec![Span::plain(format!(
                    "  {} {}: {}{}",
                    self.arrow(node),
                    window.index,
                    window.name,
                    star(window.active)
                ))]
            }
            Node::Pane(s, w, p) => {
                let pane = &self.sessions[s].windows[w].panes[p];
                let mut spans = vec![Span::plain(format!(
                    "      %{} {}{}",
                    pane.id,
                    pane.name,
                    star(pane.active)
                ))];
                if let Some(state) = pane.agent {
                    let (glyph, tone) = state_mark(state);
                    spans.push(Span::toned(format!(" {glyph}"), tone));
                }
                spans
            }
        }
    }

    /// Draws inside `area` with its border; the title counts sessions.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let title = format!("Tree · {} sessions", self.sessions.len());
        widgets::draw_box(out, area, &title, HINTS)?;
        let prompt = self.filter.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        let items: Vec<Vec<Span>> = self
            .rows()
            .into_iter()
            .map(|n| self.item_spans(n))
            .collect();
        widgets::draw_items(out, rows, &items, self.list.cursor, &self.list)
    }
}
