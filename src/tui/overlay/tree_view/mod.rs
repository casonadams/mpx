//! Choose-tree: sessions, their windows and their panes across every reachable mpx session.
mod render;

use std::collections::BTreeSet;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use crate::client::Client;
use crate::config::home_dir;
use crate::ipc::list_sessions;
use crate::layout::WindowTab;
use crate::protocol::{NodeInfo, PaneId};
use crate::tui::command::{Command, PaneCommand, SessionCommand, WindowCommand};
use crate::tui::overlay::nav::{Filter, ListState, route_list_key};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const LAYOUT_WIDTH: u16 = 200;
const LAYOUT_HEIGHT: u16 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSession {
    pub name: String,
    pub endpoint: String,
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
    pub active: bool,
    /// Foreground program, else the program the pane was spawned with.
    pub program: String,
    pub cwd: Option<String>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub last_output_ms: u64,
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
    home: Option<String>,
    collapsed: BTreeSet<CollapseKey>,
    filter: Filter,
    list: ListState,
    preview_off: bool,
    /// Screen of the previewed pane, keyed by its `session:%id` target.
    screen: Option<(String, String)>,
}

/// The pane's foreground program, else the first word of its spawn command without directories.
fn program_of(node: &NodeInfo) -> String {
    if let Some(foreground) = &node.foreground {
        return foreground.clone();
    }
    let first = node.command.split_whitespace().next().unwrap_or("");
    first.rsplit(['/', '\\']).next().unwrap_or(first).to_string()
}

fn pane_from_node(node: &NodeInfo, focused: Option<&str>) -> TreePane {
    TreePane {
        name: node.name.clone(),
        id: node.id,
        active: focused == Some(node.name.as_str()),
        program: program_of(node),
        cwd: node.cwd.clone(),
        running: node.is_running(),
        exit_code: node.exit_code,
        last_output_ms: node.last_output_ms,
    }
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
                .map(|n| pane_from_node(n, focused))
                .collect(),
        })
        .collect();
    TreeSession {
        name,
        endpoint: String::new(),
        windows,
    }
}

/// Every reachable session, in socket order; sessions that fail to answer are skipped.
pub(crate) async fn load_sessions(socket_dir: &Path) -> Vec<TreeSession> {
    let mut sessions = Vec::new();
    for endpoint in list_sessions(socket_dir) {
        if let Some(session) = load_session(&endpoint.endpoint).await {
            sessions.push(session);
        }
    }
    sessions
}

pub(crate) async fn load_session(endpoint: &str) -> Option<TreeSession> {
    let mut client = Client::connect(endpoint).await.ok()?;
    let info = client.info().await.ok()?;
    let nodes = client.list().await.ok()?;
    let (panes, _, tabs) = client.layout(LAYOUT_WIDTH, LAYOUT_HEIGHT).await.ok()?;
    let focused = panes.iter().find(|p| p.2).map(|p| p.0.clone());
    Some(TreeSession {
        endpoint: endpoint.to_string(),
        ..session_from_snapshot(info.name, &nodes, &tabs, focused.as_deref())
    })
}

async fn fetch_screen(endpoint: &str, name: &str) -> Option<String> {
    let mut client = Client::connect(endpoint).await.ok()?;
    client.screen(name).await.ok()
}

impl TreeView {
    /// Builds a view over `sessions`; the `current` one is listed first.
    pub fn from_data(sessions: Vec<TreeSession>, current: &str) -> Self {
        let mut view = Self {
            current: current.to_string(),
            ..Self::default()
        };
        view.set_sessions(sessions);
        view
    }

    fn set_sessions(&mut self, mut sessions: Vec<TreeSession>) {
        sessions.sort_by_key(|s| s.name != self.current);
        self.sessions = sessions;
        let len = self.rows().len();
        self.list.clamp(len);
    }

    /// Reloads every reachable session; sessions that fail to answer are skipped.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        let sessions = load_sessions(ctx.socket_dir).await;
        self.current = ctx.session.to_string();
        self.home = home_dir().map(|h| h.to_string_lossy().into_owned());
        self.set_sessions(sessions);
        self.refresh_screen().await;
    }

    /// Fetches the preview when the selection now points at another pane than the one shown,
    /// so moving through panes previews each at once rather than at the next refresh.
    pub async fn follow_selection(&mut self) {
        let want = self.previewed().map(|(s, w, p)| self.pane_target(s, w, p));
        if !self.preview_off && want.as_deref() != self.screen.as_ref().map(|(t, _)| t.as_str()) {
            self.refresh_screen().await;
        }
    }

    async fn refresh_screen(&mut self) {
        self.screen = None;
        if self.preview_off {
            return;
        }
        let Some((s, w, p)) = self.previewed() else {
            return;
        };
        let endpoint = &self.sessions[s].endpoint;
        if let Some(text) = fetch_screen(endpoint, &self.pane(s, w, p).name).await {
            self.screen = Some((self.pane_target(s, w, p), text));
        }
    }

    /// Labels of the visible rows in display order.
    pub fn labels(&self) -> Vec<String> {
        self.rows()
            .into_iter()
            .map(|n| self.name(n).to_string())
            .collect()
    }

    pub fn selected_label(&self) -> Option<String> {
        self.selected().map(|n| self.name(n).to_string())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        let len = self.rows().len();
        if route_list_key(&mut self.filter, &mut self.list, key, len) {
            return OverlayAction::None;
        }
        self.command_for(key.code)
    }

    fn command_for(&mut self, code: KeyCode) -> OverlayAction {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => return OverlayAction::Close,
            KeyCode::Char('p') => {
                self.preview_off = !self.preview_off;
                return OverlayAction::None;
            }
            _ => {}
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

    /// Pane the selection previews, as session, window and pane indices.
    fn previewed(&self) -> Option<(usize, usize, usize)> {
        self.selected().and_then(|n| self.preview_pane(n))
    }

    /// Pane a row previews: itself, else the focused (or first) pane of its active window.
    fn preview_pane(&self, node: Node) -> Option<(usize, usize, usize)> {
        let (s, w) = match node {
            Node::Pane(s, w, p) => return Some((s, w, p)),
            Node::Window(s, w) => (s, w),
            Node::Session(s) => {
                let windows = &self.sessions[s].windows;
                (s, windows.iter().position(|w| w.active).unwrap_or(0))
            }
        };
        let panes = &self.sessions[s].windows.get(w)?.panes;
        let p = panes.iter().position(|p| p.active).unwrap_or(0);
        (p < panes.len()).then_some((s, w, p))
    }

    fn window(&self, s: usize, w: usize) -> &TreeWindow {
        &self.sessions[s].windows[w]
    }

    fn pane(&self, s: usize, w: usize, p: usize) -> &TreePane {
        &self.sessions[s].windows[w].panes[p]
    }

    fn pane_target(&self, s: usize, w: usize, p: usize) -> String {
        format!("{}:%{}", self.sessions[s].name, self.pane(s, w, p).id)
    }

    fn name(&self, node: Node) -> &str {
        match node {
            Node::Session(s) => &self.sessions[s].name,
            Node::Window(s, w) => &self.window(s, w).name,
            Node::Pane(s, w, p) => &self.pane(s, w, p).name,
        }
    }

    /// Text the filter matches: the name, plus program and directory for panes.
    fn fields(&self, node: Node) -> Vec<&str> {
        match node {
            Node::Pane(s, w, p) => {
                let pane = self.pane(s, w, p);
                vec![&pane.name, &pane.program, pane.cwd.as_deref().unwrap_or("")]
            }
            _ => vec![self.name(node)],
        }
    }

    fn children(&self, node: Node) -> Vec<Node> {
        match node {
            Node::Session(s) => (0..self.sessions[s].windows.len())
                .map(|w| Node::Window(s, w))
                .collect(),
            Node::Window(s, w) => (0..self.window(s, w).panes.len())
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
            || self
                .collapse_key(node)
                .is_none_or(|k| !self.collapsed.contains(&k))
    }

    fn has_match(&self, node: Node) -> bool {
        self.filter.matches(&self.fields(node))
            || self.children(node).into_iter().any(|c| self.has_match(c))
    }

    fn rows(&self) -> Vec<Node> {
        let mut out = Vec::new();
        for s in 0..self.sessions.len() {
            self.push_visible(Node::Session(s), &mut out);
        }
        out
    }

    fn push_visible(&self, node: Node, out: &mut Vec<Node>) {
        if !(self.filter.text.is_empty() || self.has_match(node)) {
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
        if self.children(node).is_empty() || !self.expanded(node) {
            if let Some(parent) = Self::parent(node) {
                self.select(parent);
            }
        } else if let Some(key) = self.collapse_key(node) {
            self.collapsed.insert(key);
        }
    }

    fn expand_or_activate(&mut self, node: Node) -> OverlayAction {
        match self.collapse_key(node) {
            Some(key) if !self.expanded(node) => {
                self.collapsed.remove(&key);
                OverlayAction::None
            }
            _ => self.activate(node),
        }
    }

    /// Commands that make session `s` the current one; empty when it already is.
    fn switch_to(&self, s: usize) -> Vec<Command> {
        let name = &self.sessions[s].name;
        if *name == self.current {
            Vec::new()
        } else {
            vec![Command::Session(SessionCommand::SwitchSession {
                name: name.clone(),
            })]
        }
    }

    fn activate(&self, node: Node) -> OverlayAction {
        let commands = match node {
            Node::Session(s) => self.switch_to(s),
            Node::Window(s, w) => {
                let mut commands = self.switch_to(s);
                commands.push(Command::Window(WindowCommand::SelectWindow {
                    index: self.window(s, w).index,
                }));
                commands
            }
            Node::Pane(s, w, p) => vec![Command::Pane(PaneCommand::SelectPane {
                target: self.pane_target(s, w, p),
                keep_zoom: false,
            })],
        };
        if commands.is_empty() {
            OverlayAction::Close
        } else {
            OverlayAction::Run(commands)
        }
    }

    /// Confirms the kill; windows and sessions of other sessions switch there first.
    fn kill(&self, node: Node) -> OverlayAction {
        let (prompt, then) = match node {
            Node::Session(s) => (
                format!("kill session {}?", self.sessions[s].name),
                Command::KillServer,
            ),
            Node::Window(s, w) => {
                let window = self.window(s, w);
                (
                    format!("kill window {}?", window.name),
                    Command::Window(WindowCommand::KillWindow {
                        index: Some(window.index),
                    }),
                )
            }
            Node::Pane(s, w, p) => (
                format!("kill pane {}?", self.pane(s, w, p).name),
                Command::Pane(PaneCommand::KillPane {
                    target: Some(self.pane_target(s, w, p)),
                }),
            ),
        };
        let mut commands = match node {
            Node::Pane(..) => Vec::new(),
            Node::Session(s) | Node::Window(s, _) => self.switch_to(s),
        };
        commands.push(Command::Confirm {
            prompt,
            then: Box::new(then),
        });
        OverlayAction::Run(commands)
    }
}
