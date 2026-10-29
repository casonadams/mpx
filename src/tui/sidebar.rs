//! Agents rail: while any agent runs, a thin left column with one state dot per agent, the
//! focused pane's agent in reverse video. Alt-j / Alt-k step through the agents; focused
//! (`prefix e`) it expands over the panes, without resizing them, and moving the highlight shows
//! that agent's pane live.
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use super::agent::AgentKey;
use super::app::{App, Flow, Mode};
use super::keymap::Table;
use super::overlay::nav::ListState;
use super::overlay::widgets::{age_label, fit, join_meta, now_ms, state_mark};
use crate::agents::{self, LiveAgent};
use crate::protocol::{AgentState, NodeInfo};
use crate::vt::{Cell, CellStyle, Color};

/// Columns the expanded list takes, its border included; it is drawn over the panes.
pub const SIDEBAR_COLS: u16 = 34;
/// Columns the collapsed rail takes beside the panes: a dot and a border.
pub const RAIL_COLS: u16 = 2;
/// How long the highlight rests on another session's agent before the view switches there.
pub const SWITCH_DELAY: Duration = Duration::from_millis(150);
const AGE_COLS: usize = 4;
/// Bar, gap, state glyph and gap ahead of an agent's text.
const INDENT: usize = 4;
const HINTS: &str = "j/k move  ⏎ open  esc close";
const DIM: Option<Color> = Some(Color::BrightBlack);
const ACCENT: Option<Color> = Some(Color::Cyan);

/// Columns the collapsed rail takes beside the panes: 0 (zero stolen columns).
pub fn rail_width(_has_agents: bool, _screen_w: u16) -> u16 {
    0
}

/// The collapsed rail: one state dot per agent, sessions apart by a blank row, the agent shown in
/// the focused pane in reverse video. Returns the rows and the agent on each.
pub fn rail_rows(
    agents: &[LiveAgent],
    active: Option<&AgentKey>,
    height: u16,
    border: Color,
) -> (Vec<Vec<Cell>>, Vec<Option<AgentKey>>) {
    let mut keys: Vec<Option<AgentKey>> = Vec::new();
    let mut dots: Vec<Cell> = Vec::new();
    for (i, agent) in agents.iter().enumerate() {
        if i > 0 && agents[i - 1].session != agent.session {
            keys.push(None);
            dots.push(Cell::default());
        }
        let key = key_of(agent);
        let state = state_of(&agent.node);
        dots.push(Cell {
            c: state_mark(state).0,
            style: CellStyle {
                fg: state_color(state),
                reverse: active == Some(&key),
                ..CellStyle::default()
            },
        });
        keys.push(Some(key));
    }
    keys.resize(height as usize, None);
    dots.resize(height as usize, Cell::default());
    let edge = Cell {
        c: '│',
        style: CellStyle {
            fg: Some(border),
            ..CellStyle::default()
        },
    };
    let rows = dots.into_iter().map(|dot| vec![dot, edge]).collect();
    (rows, keys)
}

/// An agent's headline: its terminal title without leading status glyphs, else its pane name.
pub fn headline(node: &NodeInfo) -> String {
    agents::title_of(node).unwrap_or_else(|| node.name.clone())
}

/// Agents in display order: the current session first, then the others by name; panes by id.
pub fn ordered(mut agents: Vec<LiveAgent>, current: &str) -> Vec<LiveAgent> {
    agents.sort_by(|a, b| {
        (a.session != current, &a.session, a.node.id).cmp(&(
            b.session != current,
            &b.session,
            b.node.id,
        ))
    });
    agents
}

fn key_of(agent: &LiveAgent) -> AgentKey {
    (agent.session.clone(), agent.node.name.clone())
}

fn state_of(node: &NodeInfo) -> AgentState {
    if !node.is_running() {
        return AgentState::Exited;
    }
    node.agent.as_ref().map_or(AgentState::Idle, |a| a.state)
}

fn state_color(state: AgentState) -> Option<Color> {
    match state {
        AgentState::NeedsInput => Some(Color::Yellow),
        AgentState::Working => Some(Color::Green),
        AgentState::Idle => None,
        AgentState::Exited => Some(Color::Red),
    }
}

/// A styled stretch of text in a sidebar row.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: CellStyle,
}

fn run(text: impl Into<String>, fg: Option<Color>, bold: bool) -> Run {
    Run {
        text: text.into(),
        style: CellStyle {
            fg,
            bold,
            ..CellStyle::default()
        },
    }
}

/// One sidebar row, and the agent a click on it selects.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub runs: Vec<Run>,
    pub agent: Option<AgentKey>,
}

impl Line {
    fn new(runs: Vec<Run>) -> Self {
        Self { runs, agent: None }
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Exactly `width` cells: the runs clipped, then blanks.
    fn cells(&self, width: usize) -> Vec<Cell> {
        let mut cells: Vec<Cell> = self
            .runs
            .iter()
            .flat_map(|r| r.text.chars().map(|c| Cell { c, style: r.style }))
            .take(width)
            .collect();
        cells.resize(width, Cell::default());
        cells
    }
}

/// What the sidebar shows: the agents, which session is attached and which agent is highlighted.
pub struct View<'a> {
    pub agents: &'a [LiveAgent],
    pub current: &'a str,
    pub highlight: Option<&'a AgentKey>,
    pub focused: bool,
    pub now_ms: u64,
}

fn tally_runs<'a>(nodes: impl Iterator<Item = &'a NodeInfo>) -> Vec<Run> {
    let mut counts = [0usize; 3];
    for node in nodes {
        match state_of(node) {
            AgentState::NeedsInput => counts[0] += 1,
            AgentState::Working => counts[1] += 1,
            AgentState::Exited => counts[2] += 1,
            AgentState::Idle => {}
        }
    }
    [
        AgentState::NeedsInput,
        AgentState::Working,
        AgentState::Exited,
    ]
    .into_iter()
    .zip(counts)
    .filter(|(_, n)| *n > 0)
    .map(|(state, n)| run(format!(" {}{n}", state_mark(state).0), state_color(state), false))
    .collect()
}

fn session_line(name: &str, members: &[LiveAgent], current: bool, width: usize) -> Line {
    let tally = tally_runs(members.iter().map(|a| &a.node));
    let tally_w: usize = tally.iter().map(|r| r.text.chars().count()).sum();
    let fg = if current { Some(Color::Green) } else { None };
    let label = fit(name, width.saturating_sub(1 + tally_w));
    let mut runs = vec![run(" ", None, false), run(label, fg, true)];
    runs.extend(tally);
    Line::new(runs)
}

fn agent_lines(agent: &LiveAgent, view: &View, width: usize) -> [Line; 2] {
    let node = &agent.node;
    let key = key_of(agent);
    let lit = view.highlight == Some(&key);
    let state = state_of(node);
    let bar = match (lit, view.focused) {
        (true, true) => run("▌", ACCENT, true),
        (true, false) => run("▌", DIM, false),
        (false, _) => run(" ", None, false),
    };
    let text_w = width.saturating_sub(INDENT);
    let head = vec![
        bar,
        run(" ", None, false),
        run(state_mark(state).0, state_color(state), false),
        run(" ", None, false),
        run(fit(&headline(node), text_w), None, lit),
    ];
    let info = node.agent.as_ref();
    let harness = info.map_or("", |a| a.harness.as_str());
    let message = info
        .and_then(|a| a.detail.clone())
        .unwrap_or_else(|| join_meta(&[harness, &node.name]));
    let since = info.map_or(node.started_ms, |a| a.since_ms);
    let age = age_label(view.now_ms.saturating_sub(since));
    let meta_w = text_w.saturating_sub(AGE_COLS + 1);
    let meta = vec![
        run(" ".repeat(INDENT), None, false),
        run(fit(&message, meta_w), DIM, false),
        run(format!(" {age:>w$}", w = AGE_COLS), DIM, false),
    ];
    [head, meta].map(|runs| Line {
        runs,
        agent: Some(key.clone()),
    })
}

/// The scrolling body: each session's header followed by two lines per agent.
pub fn body_lines(view: &View, width: usize) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut rest = view.agents;
    while let Some(first) = rest.first() {
        let len = rest
            .iter()
            .take_while(|a| a.session == first.session)
            .count();
        let (members, tail) = rest.split_at(len);
        if !out.is_empty() {
            out.push(Line::new(Vec::new()));
        }
        let current = first.session == view.current;
        out.push(session_line(&first.session, members, current, width));
        for agent in members {
            out.extend(agent_lines(agent, view, width));
        }
        rest = tail;
    }
    if out.is_empty() {
        out.push(Line::new(vec![run("  no agents running", DIM, false)]));
    }
    out
}

/// First body line to show so the two lines from `first` fit in `height`, scrolling as little
/// as possible from `top`.
pub fn scroll_top(top: usize, first: usize, height: usize, len: usize) -> usize {
    let span = 2.min(height);
    let mut top = top.min(len.saturating_sub(height));
    if first < top {
        top = first;
    } else if first + span > top + height {
        top = first + span - height;
    }
    top
}

/// Rail state kept by the app between frames.
#[derive(Default)]
pub struct Sidebar {
    /// Agents of every reachable session, in display order.
    pub agents: Vec<LiveAgent>,
    /// The highlighted agent while the sidebar has focus.
    pub selected: Option<AgentKey>,
    /// Another session's agent to show once its delay has passed.
    pub pending: Option<(AgentKey, Instant)>,
    /// The rail had focus when the prefix was pressed: it stays expanded, and `prefix e`
    /// collapses it.
    pub resume: bool,
    list: ListState,
    top: usize,
    /// Agent under each drawn row, for mouse clicks.
    rows: Vec<Option<AgentKey>>,
}

impl Sidebar {
    pub fn keys(&self) -> Vec<AgentKey> {
        self.agents.iter().map(key_of).collect()
    }

    /// When a pending switch to another session is due.
    pub fn due(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(_, at)| *at)
    }

    /// The agent `from` steps to: the next or previous one, wrapping around; the first or last
    /// when `from` is not listed.
    pub fn step_from(&self, from: Option<&AgentKey>, next: bool) -> Option<AgentKey> {
        let keys = self.keys();
        let at = from.and_then(|from| keys.iter().position(|k| k == from));
        let idx = match at {
            Some(idx) => super::panes::wrap_step(idx, keys.len(), next),
            None if next => 0,
            None => keys.len().checked_sub(1)?,
        };
        keys.get(idx).cloned()
    }

    /// Highlights `active` when it is a listed agent, else keeps a still-listed selection, else
    /// takes the first agent.
    pub fn select_from(&mut self, active: Option<AgentKey>) {
        let keys = self.keys();
        let listed = |k: &Option<AgentKey>| k.as_ref().is_some_and(|k| keys.contains(k));
        self.selected = if listed(&active) {
            active
        } else if listed(&self.selected) {
            self.selected.take()
        } else {
            keys.first().cloned()
        };
    }

    /// Applies a list motion. `None` when `key` is not a motion; otherwise whether the selection
    /// moved to another agent.
    pub fn motion(&mut self, key: KeyEvent) -> Option<bool> {
        let keys = self.keys();
        self.list.cursor = self
            .selected
            .as_ref()
            .and_then(|s| keys.iter().position(|k| k == s))
            .unwrap_or(0);
        if !self.list.motion(key, keys.len()) {
            return None;
        }
        let next = keys.get(self.list.cursor).cloned();
        let moved = next.is_some() && next != self.selected;
        self.selected = next.or(self.selected.take());
        Some(moved)
    }

    /// The agent drawn on screen row `row` in the last frame.
    pub fn agent_at_row(&self, row: u16) -> Option<AgentKey> {
        self.rows.get(row as usize).cloned().flatten()
    }
}

/// A drawn sidebar: its rows of cells, the agent under each row and the body scroll position.
pub struct Rendered {
    pub rows: Vec<Vec<Cell>>,
    pub agents: Vec<Option<AgentKey>>,
    pub top: usize,
}

/// The sidebar's rows for a `(width, height)` column whose right border is drawn in `border`,
/// scrolled from `top` just enough to keep the highlighted agent in view.
pub fn render(view: &View, top: usize, (width, height): (u16, u16), border: Color) -> Rendered {
    let inner = (width as usize).saturating_sub(1);
    let height = height as usize;
    let mut header = vec![run(" Agents", ACCENT, true)];
    header.extend(tally_runs(view.agents.iter().map(|a| &a.node)));
    let footer = view
        .focused
        .then(|| Line::new(vec![run(format!(" {HINTS}"), DIM, false)]));
    let body = body_lines(view, inner);
    let body_h = height.saturating_sub(1 + usize::from(footer.is_some()));
    let first = view
        .highlight
        .and_then(|k| body.iter().position(|l| l.agent.as_ref() == Some(k)));
    let top = match first {
        Some(first) => scroll_top(top, first, body_h, body.len()),
        None => top.min(body.len().saturating_sub(body_h)),
    };
    let mut lines = vec![Line::new(header)];
    lines.extend(body.into_iter().skip(top).take(body_h));
    lines.resize(
        height.saturating_sub(usize::from(footer.is_some())),
        Line::new(Vec::new()),
    );
    lines.extend(footer);
    lines.truncate(height);
    let edge = Cell {
        c: '│',
        style: CellStyle {
            fg: Some(border),
            ..CellStyle::default()
        },
    };
    let rows = lines
        .iter()
        .map(|line| {
            let mut cells = line.cells(inner);
            cells.push(edge);
            cells
        })
        .collect();
    Rendered {
        rows,
        agents: lines.into_iter().map(|l| l.agent).collect(),
        top,
    }
}

impl App {
    /// Focuses the vertical drawer, expanding it; `prefix .` again collapses it.
    pub async fn toggle_sidebar(&mut self) -> Flow {
        if matches!(self.mode, Mode::Sidebar) || self.sidebar.resume {
            self.mode = Mode::Normal;
            return Flow::Continue;
        }
        self.refresh_sidebar().await;
        if self.sidebar.agents.is_empty() {
            return self.say("no agents running");
        }
        if self.viewport.0 < SIDEBAR_COLS {
            return self.say("screen too narrow for the agents drawer");
        }
        self.focus_sidebar();
        Flow::Continue
    }

    /// Shows the next or previous agent (Alt-j / Alt-k) without focusing the rail.
    pub async fn step_agent(&mut self, next: bool) -> Flow {
        let from = match (&self.sidebar.pending, matches!(self.mode, Mode::Sidebar)) {
            (Some((pending, _)), _) => Some(pending.clone()),
            (None, true) => self.sidebar.selected.clone(),
            (None, false) => Some((self.session.clone(), self.active_pane.clone())),
        };
        let Some(key) = self.sidebar.step_from(from.as_ref(), next) else {
            return self.say("no agents running");
        };
        self.sidebar.selected = Some(key);
        self.show_selected().await;
        Flow::Continue
    }

    /// Reloads the agent list from every reachable session.
    pub async fn refresh_sidebar(&mut self) {
        let live = agents::list_live(&self.socket_dir).await;
        self.sidebar.agents = ordered(live, &self.session);
    }

    /// Moves keyboard focus to the sidebar, highlighting the agent on screen when there is one.
    pub fn focus_sidebar(&mut self) {
        self.park_copy();
        let active = (self.session.clone(), self.active_pane.clone());
        self.sidebar.select_from(Some(active));
        self.mode = Mode::Sidebar;
    }

    pub async fn sidebar_key(&mut self, key: KeyEvent) -> Flow {
        if self.keymap.is_prefix(&key) {
            self.sidebar.resume = true;
            self.enter_prefix();
            return Flow::Continue;
        }
        if let Some(binding) = self.keymap.lookup(Table::Root, &key) {
            let cmd = binding.command.clone();
            self.mode = Mode::Sidebar;
            return self.dispatch(cmd).await;
        }
        if let Some(moved) = self.sidebar.motion(key) {
            if moved {
                self.show_selected().await;
            }
            self.mode = Mode::Sidebar;
            return Flow::Continue;
        }
        match key.code {
            KeyCode::Enter
            | KeyCode::Char('l')
            | KeyCode::Right
            | KeyCode::Esc
            | KeyCode::Char('q') => self.leave_sidebar().await,
            _ => self.mode = Mode::Sidebar,
        }
        Flow::Continue
    }

    /// Shows the selected agent: at once in this session, after a short rest in another.
    async fn show_selected(&mut self) {
        let Some(key) = self.sidebar.selected.clone() else {
            return;
        };
        if key.0 == self.session {
            self.sidebar.pending = None;
            self.show_agent(key).await;
        } else {
            self.sidebar.pending = Some((key, Instant::now() + SWITCH_DELAY));
        }
    }

    /// Applies a pending switch whose delay has passed; called when its deadline wakes the loop.
    pub async fn on_due(&mut self) -> Flow {
        if self.sidebar.due().is_some_and(|at| Instant::now() >= at) {
            self.flush_pending().await;
        }
        Flow::Continue
    }

    /// Collapses the rail and hands the keys to the agent it shows, switching to another session
    /// first if the highlight is waiting to.
    pub async fn leave_sidebar(&mut self) {
        self.mode = Mode::Normal;
        self.flush_pending().await;
    }

    async fn flush_pending(&mut self) {
        if let Some((key, _)) = self.sidebar.pending.take() {
            self.show_agent(key).await;
        }
    }

    /// Attaches to the agent's session when needed and focuses its pane, keeping sidebar focus.
    async fn show_agent(&mut self, (session, pane): AgentKey) {
        let in_sidebar = matches!(self.mode, Mode::Sidebar);
        if let Err(e) = self.switch_session(&session).await {
            self.say(e);
            return;
        }
        self.zoomed = None;
        self.layout.dirty = true;
        let result = self.client.focus(&pane).await;
        self.settle(result);
        if in_sidebar {
            self.mode = Mode::Sidebar;
        }
    }

    /// A click on a sidebar row focuses the sidebar and shows the agent drawn there.
    pub async fn sidebar_click(&mut self, row: u16) {
        self.focus_sidebar();
        if let Some(key) = self.sidebar.agent_at_row(row) {
            self.sidebar.selected = Some(key.clone());
            self.sidebar.pending = None;
            self.show_agent(key).await;
        }
    }

    /// Columns the rail takes beside the panes on the current screen.
    pub fn sidebar_width(&self) -> u16 {
        0
    }

    /// Columns the expanded list covers, or 0 while it is collapsed.
    pub fn expanded_width(&self) -> u16 {
        let focused = matches!(self.mode, Mode::Sidebar) || self.sidebar.resume;
        if !focused || self.viewport.0 < SIDEBAR_COLS {
            return 0;
        }
        SIDEBAR_COLS.min(self.viewport.0)
    }

    /// The collapsed rail's rows for the frame being drawn; the reverse dot is the shown agent,
    /// or the one a step is about to switch sessions to.
    pub fn rail(&mut self, border: Color) -> Vec<Vec<Cell>> {
        let shown = match &self.sidebar.pending {
            Some((key, _)) => key.clone(),
            None => (self.session.clone(), self.active_pane.clone()),
        };
        let (rows, keys) = rail_rows(&self.sidebar.agents, Some(&shown), self.viewport.1, border);
        self.sidebar.rows = keys;
        rows
    }

    /// The expanded list's rows for the frame being drawn, highlighting the selection while
    /// focused or peeking and the attached pane otherwise.
    pub fn sidebar_rows(&mut self, border: Color) -> Vec<Vec<Cell>> {
        let focused = matches!(self.mode, Mode::Sidebar);
        let active = (self.session.clone(), self.active_pane.clone());
        let highlight = if focused {
            self.sidebar.selected.clone()
        } else {
            Some(active)
        };
        let size = (self.expanded_width(), self.viewport.1);
        let view = View {
            agents: &self.sidebar.agents,
            current: &self.session,
            highlight: highlight.as_ref(),
            focused,
            now_ms: now_ms(),
        };
        let drawn = render(&view, self.sidebar.top, size, border);
        self.sidebar.top = drawn.top;
        self.sidebar.rows = drawn.agents;
        drawn.rows
    }
}
