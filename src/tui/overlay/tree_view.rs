//! Choose-tree: sessions, their windows and their panes across every reachable mpx session.
use std::collections::BTreeSet;
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::client::Client;
use crate::config::home_dir;
use crate::ipc::list_sessions;
use crate::layout::{Rect, WindowTab};
use crate::protocol::{NodeInfo, PaneId};
use crate::tui::command::Command;
use crate::tui::overlay::nav::{Filter, FilterEvent, ListState, has_ctrl};
use crate::tui::overlay::widgets::{
    self, Span, Tone, age_label, fit, join_meta, now_ms, short_path, tail_lines,
};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const HINTS: &str = "j/k move  l/Enter open  h collapse  / filter  x kill  p preview  q close";
const LAYOUT_WIDTH: u16 = 200;
const LAYOUT_HEIGHT: u16 = 50;
const PREVIEW_MIN_HEIGHT: u16 = 14;
const TAG_COLS: usize = 10;
const DETAIL_COLS: usize = 24;
/// Rows narrower than this drop the detail column.
const DETAIL_MIN_WIDTH: usize = 72;
const TALLY_COLS: usize = 9;
const AGE_COLS: usize = 4;
const FRESH_MS: u64 = 60_000;

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

/// Agent states and exits counted over a set of panes.
#[derive(Debug, Default)]
struct Tally {
    panes: usize,
    exited: usize,
    last_output_ms: u64,
}

impl Tally {
    fn of<'a>(panes: impl Iterator<Item = &'a TreePane>) -> Self {
        let mut tally = Tally::default();
        for pane in panes {
            tally.panes += 1;
            tally.last_output_ms = tally.last_output_ms.max(pane.last_output_ms);
            if !pane.running {
                tally.exited += 1;
            }
        }
        tally
    }

    fn spans(&self) -> Vec<Span> {
        if self.exited > 0 {
            vec![Span::toned(format!("{} exited", self.exited), Tone::Failed)]
        } else {
            Vec::new()
        }
    }
}

/// One rendered row before it is laid out into columns.
struct Cells {
    lead: Vec<Span>,
    name: Span,
    tag: Span,
    detail: String,
    tally: Vec<Span>,
    last_output_ms: u64,
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Program name of a spawn command line: its first word without directories.
fn program_of(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or("");
    first.rsplit(['/', '\\']).next().unwrap_or(first).to_string()
}

fn pane_from_node(node: &NodeInfo, focused: Option<&str>) -> TreePane {
    TreePane {
        name: node.name.clone(),
        id: node.id,
        active: focused == Some(node.name.as_str()),
        program: node
            .foreground
            .clone()
            .unwrap_or_else(|| program_of(&node.command)),
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
        let mut sessions = Vec::new();
        for endpoint in list_sessions(ctx.socket_dir) {
            if let Some(session) = load_session(&endpoint.endpoint).await {
                sessions.push(session);
            }
        }
        self.current = ctx.session.to_string();
        self.home = home_dir().map(|h| h.to_string_lossy().into_owned());
        self.set_sessions(sessions);
        self.refresh_screen().await;
    }

    /// Fetches the preview when the selection now points at another pane than the one shown,
    /// so moving through panes previews each at once rather than at the next refresh.
    pub async fn follow_selection(&mut self) {
        let want = self
            .selected()
            .and_then(|n| self.preview_pane(n))
            .map(|(s, w, p)| self.pane_target(s, w, p));
        if !self.preview_off && want != self.screen.as_ref().map(|(shown, _)| shown.clone()) {
            self.refresh_screen().await;
        }
    }

    async fn refresh_screen(&mut self) {
        self.screen = None;
        if self.preview_off {
            return;
        }
        let Some((s, w, p)) = self.selected().and_then(|n| self.preview_pane(n)) else {
            return;
        };
        let session = &self.sessions[s];
        let pane = &session.windows[w].panes[p];
        if let Some(text) = fetch_screen(&session.endpoint, &pane.name).await {
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

    pub fn selected_label(&self) -> Option<String> {
        self.selected().map(|n| self.name(n).to_string())
    }
    fn name(&self, node: Node) -> &str {
        match node {
            Node::Session(s) => &self.sessions[s].name,
            Node::Window(s, w) => &self.sessions[s].windows[w].name,
            Node::Pane(s, w, p) => &self.sessions[s].windows[w].panes[p].name,
        }
    }

    /// Text the filter matches: the name, plus program and directory for panes.
    fn fields(&self, node: Node) -> Vec<&str> {
        match node {
            Node::Pane(s, w, p) => {
                let pane = &self.sessions[s].windows[w].panes[p];
                vec![
                    &pane.name,
                    &pane.program,
                    pane.cwd.as_deref().unwrap_or(""),
                ]
            }
            _ => vec![self.name(node)],
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
        self.filter.matches(&self.fields(node))
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

    fn arrow(&self, node: Node) -> char {
        if self.children(node).is_empty() {
            ' '
        } else if self.expanded(node) {
            '▾'
        } else {
            '▸'
        }
    }


    fn cells(&self, node: Node, selected: bool) -> Cells {
        match node {
            Node::Session(s) => self.session_cells(node, s, selected),
            Node::Window(s, w) => self.window_cells(node, &self.sessions[s].windows[w], selected),
            Node::Pane(s, w, p) => self.pane_cells(&self.sessions[s].windows[w].panes[p], selected),
        }
    }

    fn session_cells(&self, node: Node, s: usize, selected: bool) -> Cells {
        let session = &self.sessions[s];
        let current = session.name == self.current;
        let tally = Tally::of(session.windows.iter().flat_map(|w| &w.panes));
        let tone = if current { Tone::Active } else { Tone::Plain };
        let name = if current || selected {
            Span::bold(session.name.clone(), tone)
        } else {
            Span::plain(session.name.clone())
        };
        let tag = if current {
            Span::toned("current", Tone::Active)
        } else {
            Span::plain("")
        };
        Cells {
            lead: vec![Span::plain(format!("{} ", self.arrow(node)))],
            name,
            tag,
            detail: format!(
                "{} · {}",
                plural(session.windows.len(), "window"),
                plural(tally.panes, "pane")
            ),
            tally: tally.spans(),
            last_output_ms: tally.last_output_ms,
        }
    }

    fn window_cells(&self, node: Node, window: &TreeWindow, selected: bool) -> Cells {
        let tally = Tally::of(window.panes.iter());
        let label = format!("{} {}", window.index, window.name);
        Cells {
            lead: vec![Span::plain(format!("  {} ", self.arrow(node)))],
            name: if selected {
                Span::bold(label, Tone::Plain)
            } else {
                Span::plain(label)
            },
            tag: Span::dim(if window.active { "active" } else { "" }),
            detail: plural(tally.panes, "pane"),
            tally: tally.spans(),
            last_output_ms: tally.last_output_ms,
        }
    }

    fn pane_cells(&self, pane: &TreePane, selected: bool) -> Cells {
        let focus = if pane.active { '›' } else { ' ' };
        let (glyph, tone) = if pane.running {
            ('●', Tone::Accent)
        } else {
            ('×', Tone::Failed)
        };
        let name = match (selected, pane.running) {
            (true, _) => Span::bold(pane.name.clone(), Tone::Plain),
            (false, true) => Span::plain(pane.name.clone()),
            (false, false) => Span::dim(pane.name.clone()),
        };
        let tag = match (pane.running, pane.exit_code) {
            (true, _) => Span::toned(pane.program.clone(), Tone::Accent),
            (false, Some(code)) => Span::toned(format!("exit {code}"), Tone::Failed),
            (false, None) => Span::toned("exited", Tone::Failed),
        };
        let cwd = pane.cwd.as_deref().unwrap_or("");
        Cells {
            lead: vec![
                Span::toned(format!("     {focus}"), Tone::Accent),
                Span::toned(format!("{glyph} "), tone),
            ],
            name,
            tag,
            detail: short_path(cwd, self.home.as_deref()),
            tally: Vec::new(),
            last_output_ms: pane.last_output_ms,
        }
    }

    /// Lays `cells` out as: bar, lead, name, tag, detail, tally, age.
    fn row_spans(cells: Cells, width: usize, selected: bool) -> Vec<Span> {
        let detail_w = if width >= DETAIL_MIN_WIDTH {
            DETAIL_COLS + 1
        } else {
            0
        };
        let lead_w: usize = cells.lead.iter().map(|s| s.text.chars().count()).sum();
        let fixed = 1 + lead_w + 1 + TAG_COLS + 1 + detail_w + TALLY_COLS + AGE_COLS;
        let name_w = width.saturating_sub(fixed);
        let bar = if selected {
            Span::bold("▌", Tone::Accent)
        } else {
            Span::plain(" ")
        };
        let mut spans = vec![bar];
        spans.extend(cells.lead);
        spans.push(Span {
            text: fit(&cells.name.text, name_w),
            ..cells.name
        });
        spans.push(Span::plain(" "));
        spans.push(Span {
            text: format!("{} ", fit(&cells.tag.text, TAG_COLS)),
            ..cells.tag
        });
        if detail_w > 0 {
            spans.push(Span::dim(format!("{} ", fit(&cells.detail, DETAIL_COLS))));
        }
        let tally_w: usize = cells.tally.iter().map(|s| s.text.chars().count()).sum();
        spans.extend(cells.tally);
        spans.push(Span::plain(" ".repeat(TALLY_COLS.saturating_sub(tally_w))));
        spans.push(age_span(cells.last_output_ms));
        spans
    }

    fn title(&self) -> String {
        format!("Sessions · {}", self.sessions.len())
    }

    /// Draws inside `area`: the tree, plus a preview of the selected pane when there is room.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let (list_area, preview_area) = self.split_area(area);
        self.render_list(out, list_area)?;
        match preview_area {
            Some(preview) => self.render_preview(out, preview),
            None => Ok(()),
        }
    }

    fn split_area(&self, area: Rect) -> (Rect, Option<Rect>) {
        if self.preview_off || self.filter.editing || area.height < PREVIEW_MIN_HEIGHT {
            return (area, None);
        }
        let (list, preview) = widgets::split_rows(area, area.height / 2);
        (list, Some(preview))
    }

    fn render_list<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::draw_panel(out, area, &self.title(), HINTS)?;
        let prompt = self.filter.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        let nodes = self.rows();
        if nodes.is_empty() {
            return widgets::put_row(out, rows, 0, &[Span::dim("no sessions")], false);
        }
        let width = rows.width as usize;
        let cursor = self.list.cursor;
        for (row, idx) in self
            .list
            .window(cursor, nodes.len(), rows.height as usize)
            .enumerate()
        {
            let on = idx == cursor;
            let spans = Self::row_spans(self.cells(nodes[idx], on), width, on);
            widgets::put_row(out, rows, row as u16, &spans, false)?;
        }
        Ok(())
    }

    fn render_preview<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let target = self.selected().and_then(|n| self.preview_pane(n));
        let Some((s, w, p)) = target else {
            return widgets::draw_preview(out, area, None, &[], None);
        };
        let pane = &self.sessions[s].windows[w].panes[p];
        let key = self.pane_target(s, w, p);
        let cwd = short_path(pane.cwd.as_deref().unwrap_or(""), self.home.as_deref());
        let meta = vec![
            Span::bold(pane.name.clone(), Tone::Plain),
            Span::dim(format!(" · {}", join_meta(&[&pane.program, &cwd, &key]))),
        ];
        let text = match &self.screen {
            Some((k, text)) if *k == key => text.as_str(),
            _ => "",
        };
        let lines: Vec<Span> = tail_lines(text, widgets::preview_height(area, true))
            .into_iter()
            .map(Span::plain)
            .collect();
        widgets::draw_preview(out, area, Some(meta), &lines, Some("nothing to preview yet"))
    }
}

fn age_span(last_ms: u64) -> Span {
    if last_ms == 0 {
        return Span::plain(" ".repeat(AGE_COLS));
    }
    let elapsed = now_ms().saturating_sub(last_ms);
    let age = format!("{:>w$}", age_label(elapsed), w = AGE_COLS);
    if elapsed < FRESH_MS {
        Span::plain(age)
    } else {
        Span::dim(age)
    }
}
