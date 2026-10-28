//! Agent session manager: live agents grouped by state, then resumable sessions for the scope.
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use crate::agents::{LiveAgent, ResumableSession, list_live, scan_resumable, transcript_preview};
use crate::client::Client;
use crate::layout::Rect;
use crate::protocol::AgentState;
use crate::tui::command::Command;
use crate::tui::overlay::nav::{Filter, FilterEvent, ListState, has_ctrl};
use crate::tui::overlay::widgets::{self, Span, Tone, age_label, fit, now_ms, state_mark};
use crate::tui::overlay::{OverlayAction, OverlayCtx};
use crate::tui::render::copy_to_clipboard;

const HINTS: &str = "j/k move  Enter open  r/R resume  n new  m msg  Tab mark  x kill  y yank  p preview  s scope  q close";
const RESUME_LIMIT: usize = 50;
const PREVIEW_MIN_WIDTH: u16 = 100;
const NAME_COLS: usize = 18;
const HARNESS_COLS: usize = 8;
const REF_COLS: usize = 14;
/// Lead glyph (3 cells) plus the fixed columns and their separators.
const FIXED_COLS: usize = 3 + NAME_COLS + 1 + HARNESS_COLS + 1 + REF_COLS + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Live(usize),
    Resume(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Header {
        title: String,
        glyph: char,
        tone: Tone,
    },
    Row(Slot),
}

struct Section {
    title: String,
    glyph: char,
    tone: Tone,
    slots: Vec<Slot>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Browse,
    Message,
    Harness,
}

struct Columns {
    lead: Span,
    name: String,
    harness: String,
    reference: String,
    detail: String,
    age: String,
}

const LIVE_GROUPS: [AgentState; 4] = [
    AgentState::NeedsInput,
    AgentState::Working,
    AgentState::Idle,
    AgentState::Exited,
];

fn group_title(state: AgentState) -> &'static str {
    match state {
        AgentState::NeedsInput => "NEEDS YOU",
        AgentState::Working => "WORKING",
        AgentState::Idle => "IDLE",
        AgentState::Exited => "EXITED",
    }
}

fn group_of(agent: &LiveAgent) -> AgentState {
    if !agent.node.is_running() {
        return AgentState::Exited;
    }
    agent
        .node
        .agent
        .as_ref()
        .map_or(AgentState::Idle, |a| a.state)
}

fn since(agent: &LiveAgent) -> u64 {
    agent.node.agent.as_ref().map_or(0, |a| a.since_ms)
}

fn activity(agent: &LiveAgent) -> u64 {
    agent.node.last_output_ms.max(since(agent))
}

fn agent_key(agent: &LiveAgent) -> String {
    format!("{}:%{}", agent.session, agent.node.id)
}

fn live_fields(agent: &LiveAgent) -> Vec<&str> {
    let node = &agent.node;
    let mut fields = vec![
        node.name.as_str(),
        node.title.as_deref().unwrap_or(""),
        node.cwd.as_deref().unwrap_or(""),
    ];
    if let Some(info) = &node.agent {
        fields.push(info.harness.as_str());
        fields.push(info.detail.as_deref().unwrap_or(""));
    }
    fields
}

fn tail_lines(text: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(max);
    lines[start..].iter().map(|l| l.to_string()).collect()
}

fn cursor_line(lines: &[Line], slot: Option<Slot>) -> usize {
    lines
        .iter()
        .position(|l| matches!(l, Line::Row(s) if Some(*s) == slot))
        .unwrap_or(0)
}

fn column_spans(cols: Columns, width: usize) -> Vec<Span> {
    let age_len = cols.age.chars().count();
    let detail_w = width.saturating_sub(FIXED_COLS + age_len + 1);
    vec![
        cols.lead,
        Span::plain(format!("{} ", fit(&cols.name, NAME_COLS))),
        Span::plain(format!("{} ", fit(&cols.harness, HARNESS_COLS))),
        Span::dim(format!("{} ", fit(&cols.reference, REF_COLS))),
        Span::dim(format!("{} ", fit(&cols.detail, detail_w))),
        Span::dim(cols.age),
    ]
}

fn resume_spans(session: &ResumableSession, width: usize) -> Vec<Span> {
    let age = age_label(now_ms().saturating_sub(session.modified_ms));
    column_spans(
        Columns {
            lead: Span::dim(" ◌ "),
            name: session.title.clone(),
            harness: session.harness.clone(),
            reference: session.id.clone(),
            detail: session.cwd.clone().unwrap_or_default(),
            age,
        },
        width,
    )
}

async fn fetch_screen(endpoint: &str, name: &str) -> Option<String> {
    let mut client = Client::connect(endpoint).await.ok()?;
    client.screen(name).await.ok()
}

pub struct AgentsView {
    live: Vec<LiveAgent>,
    resumable: Vec<ResumableSession>,
    harnesses: Vec<String>,
    cwd: Option<String>,
    scope_all: bool,
    preview: bool,
    filter: Filter,
    list: ListState,
    mode: Mode,
    input: String,
    harness_pick: usize,
    marks: BTreeSet<String>,
    screens: HashMap<String, String>,
}

impl Default for AgentsView {
    fn default() -> Self {
        Self {
            live: Vec::new(),
            resumable: Vec::new(),
            harnesses: Vec::new(),
            cwd: None,
            scope_all: false,
            preview: true,
            filter: Filter::default(),
            list: ListState::default(),
            mode: Mode::Browse,
            input: String::new(),
            harness_pick: 0,
            marks: BTreeSet::new(),
            screens: HashMap::new(),
        }
    }
}

impl AgentsView {
    pub fn from_data(
        live: Vec<LiveAgent>,
        resumable: Vec<ResumableSession>,
        harnesses: Vec<String>,
        cwd: Option<String>,
    ) -> Self {
        Self {
            live,
            resumable,
            harnesses,
            cwd,
            ..Self::default()
        }
    }

    /// Reloads live agents, the resumable scope and the cached preview of the selected agent.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        self.live = list_live(ctx.socket_dir).await;
        self.cwd = ctx.cwd.map(str::to_string);
        self.harnesses = ctx.registry.iter().map(|h| h.name.clone()).collect();
        let scope = if self.scope_all {
            None
        } else {
            ctx.cwd.map(PathBuf::from)
        };
        self.resumable = scan_resumable(ctx.registry, scope.as_deref(), RESUME_LIMIT);
        let live_keys: Vec<String> = self.live.iter().map(agent_key).collect();
        self.marks.retain(|m| live_keys.contains(m));
        self.refresh_screen().await;
        let len = self.slots().len();
        self.list.clamp(len);
    }

    async fn refresh_screen(&mut self) {
        self.screens.clear();
        if !self.preview {
            return;
        }
        if let Some(Slot::Live(i)) = self.selected() {
            let key = agent_key(&self.live[i]);
            let (endpoint, name) = (
                self.live[i].endpoint.clone(),
                self.live[i].node.name.clone(),
            );
            if let Some(text) = fetch_screen(&endpoint, &name).await {
                self.screens.insert(key, text);
            }
        }
    }

    pub fn scope_all(&self) -> bool {
        self.scope_all
    }

    /// Session id that `y` copies for the selected row.
    pub fn yank_text(&self) -> Option<String> {
        match self.selected()? {
            Slot::Live(i) => self.live[i].node.agent.as_ref()?.session_id.clone(),
            Slot::Resume(i) => Some(self.resumable[i].id.clone()),
        }
    }

    /// Section titles in display order, e.g. `NEEDS YOU`, `RESUME · all`.
    pub fn group_titles(&self) -> Vec<String> {
        self.sections().into_iter().map(|s| s.title).collect()
    }

    /// Names of the visible rows in display order (live node name, or resumable session id).
    pub fn visible_names(&self) -> Vec<String> {
        self.slots()
            .iter()
            .map(|slot| self.slot_name(*slot))
            .collect()
    }

    fn slot_name(&self, slot: Slot) -> String {
        match slot {
            Slot::Live(i) => self.live[i].node.name.clone(),
            Slot::Resume(i) => self.resumable[i].id.clone(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match self.mode {
            Mode::Browse => self.browse_key(key),
            Mode::Message => self.message_key(key),
            Mode::Harness => self.harness_key(key),
        }
    }

    fn browse_key(&mut self, key: KeyEvent) -> OverlayAction {
        match self.filter.handle(key) {
            FilterEvent::Edited => {
                self.list.cursor = 0;
                return OverlayAction::None;
            }
            FilterEvent::Consumed => return OverlayAction::None,
            FilterEvent::Ignored => {}
        }
        let len = self.slots().len();
        if self.list.motion(key, len) || has_ctrl(key) {
            return OverlayAction::None;
        }
        self.command_for(key.code)
    }

    fn command_for(&mut self, code: KeyCode) -> OverlayAction {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Enter => self.open_selected(),
            KeyCode::Tab => self.toggle_mark(),
            KeyCode::Char('r') => self.resume_selected(false),
            KeyCode::Char('R') => self.resume_selected(true),
            KeyCode::Char('n') => self.start_harness_pick(),
            KeyCode::Char('m') => self.start_message(),
            KeyCode::Char('x') => self.kill_selected(),
            KeyCode::Char('y') => {
                if let Some(id) = self.yank_text() {
                    copy_to_clipboard(&id);
                }
                OverlayAction::None
            }
            KeyCode::Char('p') => {
                self.preview = !self.preview;
                OverlayAction::None
            }
            KeyCode::Char('s') => {
                self.scope_all = !self.scope_all;
                OverlayAction::None
            }
            _ => OverlayAction::None,
        }
    }

    fn selected(&self) -> Option<Slot> {
        self.slots().get(self.list.cursor).copied()
    }

    fn open_selected(&self) -> OverlayAction {
        match self.selected() {
            Some(Slot::Live(i)) => OverlayAction::Run(vec![Command::SelectPane {
                target: agent_key(&self.live[i]),
            }]),
            Some(Slot::Resume(i)) => self.launch(i, false),
            None => OverlayAction::None,
        }
    }

    fn resume_selected(&self, split: bool) -> OverlayAction {
        match self.selected() {
            Some(Slot::Resume(i)) => self.launch(i, split),
            _ => OverlayAction::None,
        }
    }

    fn launch(&self, index: usize, split: bool) -> OverlayAction {
        let session = &self.resumable[index];
        OverlayAction::Run(vec![Command::LaunchAgent {
            harness: session.harness.clone(),
            resume: Some(session.id.clone()),
            cwd: session.cwd.clone(),
            split,
        }])
    }

    fn kill_selected(&self) -> OverlayAction {
        let Some(Slot::Live(i)) = self.selected() else {
            return OverlayAction::None;
        };
        let agent = &self.live[i];
        OverlayAction::Run(vec![Command::Confirm {
            prompt: format!("kill {}?", agent.node.name),
            then: Box::new(Command::KillPane {
                target: Some(agent_key(agent)),
            }),
        }])
    }

    fn toggle_mark(&mut self) -> OverlayAction {
        if let Some(Slot::Live(i)) = self.selected() {
            let key = agent_key(&self.live[i]);
            if !self.marks.remove(&key) {
                self.marks.insert(key);
            }
        }
        OverlayAction::None
    }

    fn start_harness_pick(&mut self) -> OverlayAction {
        self.mode = Mode::Harness;
        self.harness_pick = 0;
        OverlayAction::None
    }

    fn start_message(&mut self) -> OverlayAction {
        self.mode = Mode::Message;
        self.input.clear();
        OverlayAction::None
    }

    fn harness_key(&mut self, key: KeyEvent) -> OverlayAction {
        let last = self.harnesses.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.mode = Mode::Browse,
            KeyCode::Char('j') | KeyCode::Down => {
                self.harness_pick = (self.harness_pick + 1).min(last)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.harness_pick = self.harness_pick.saturating_sub(1)
            }
            KeyCode::Enter => return self.launch_new(),
            _ => {}
        }
        OverlayAction::None
    }

    fn launch_new(&mut self) -> OverlayAction {
        self.mode = Mode::Browse;
        match self.harnesses.get(self.harness_pick) {
            Some(harness) => OverlayAction::Run(vec![Command::LaunchAgent {
                harness: harness.clone(),
                resume: None,
                cwd: self.cwd.clone(),
                split: false,
            }]),
            None => OverlayAction::None,
        }
    }

    fn message_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
                OverlayAction::None
            }
            KeyCode::Enter => self.send_message(),
            KeyCode::Backspace => {
                self.input.pop();
                OverlayAction::None
            }
            KeyCode::Char(c) if !has_ctrl(key) => {
                self.input.push(c);
                OverlayAction::None
            }
            _ => OverlayAction::None,
        }
    }

    /// Marked live agents, or the selected live agent when nothing is marked.
    fn message_targets(&self) -> Vec<String> {
        let marked: Vec<String> = self
            .live
            .iter()
            .map(agent_key)
            .filter(|key| self.marks.contains(key))
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        match self.selected() {
            Some(Slot::Live(i)) => vec![agent_key(&self.live[i])],
            _ => Vec::new(),
        }
    }

    fn send_message(&mut self) -> OverlayAction {
        let text = std::mem::take(&mut self.input);
        self.mode = Mode::Browse;
        let targets = self.message_targets();
        if text.is_empty() || targets.is_empty() {
            return OverlayAction::None;
        }
        OverlayAction::Run(
            targets
                .into_iter()
                .map(|target| Command::Paste {
                    target: Some(target),
                    text: text.clone(),
                    submit: true,
                })
                .collect(),
        )
    }

    fn live_matches(&self, agent: &LiveAgent) -> bool {
        self.filter.matches(&live_fields(agent))
    }

    fn resume_matches(&self, session: &ResumableSession) -> bool {
        self.filter.matches(&[
            session.id.as_str(),
            session.title.as_str(),
            session.harness.as_str(),
            session.cwd.as_deref().unwrap_or(""),
        ])
    }

    fn scope_label(&self) -> &str {
        match (self.scope_all, &self.cwd) {
            (false, Some(cwd)) => cwd,
            _ => "all",
        }
    }

    fn live_section(&self, state: AgentState) -> Option<Section> {
        let mut idx: Vec<usize> = (0..self.live.len())
            .filter(|&i| group_of(&self.live[i]) == state && self.live_matches(&self.live[i]))
            .collect();
        if state == AgentState::NeedsInput {
            idx.sort_by_key(|&i| since(&self.live[i]));
        } else {
            idx.sort_by_key(|&i| Reverse(activity(&self.live[i])));
        }
        let (glyph, tone) = state_mark(state);
        (!idx.is_empty()).then(|| Section {
            title: group_title(state).to_string(),
            glyph,
            tone,
            slots: idx.into_iter().map(Slot::Live).collect(),
        })
    }

    fn resume_section(&self) -> Option<Section> {
        let slots: Vec<Slot> = (0..self.resumable.len())
            .filter(|&i| self.resume_matches(&self.resumable[i]))
            .map(Slot::Resume)
            .collect();
        (!slots.is_empty()).then(|| Section {
            title: format!("RESUME · {}", self.scope_label()),
            glyph: '◌',
            tone: Tone::Plain,
            slots,
        })
    }

    fn sections(&self) -> Vec<Section> {
        let mut sections: Vec<Section> = LIVE_GROUPS
            .iter()
            .filter_map(|&s| self.live_section(s))
            .collect();
        sections.extend(self.resume_section());
        sections
    }

    fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        for section in self.sections() {
            lines.push(Line::Header {
                title: section.title,
                glyph: section.glyph,
                tone: section.tone,
            });
            lines.extend(section.slots.into_iter().map(Line::Row));
        }
        lines
    }

    fn slots(&self) -> Vec<Slot> {
        self.lines()
            .into_iter()
            .filter_map(|line| match line {
                Line::Row(slot) => Some(slot),
                Line::Header { .. } => None,
            })
            .collect()
    }

    fn need_count(&self) -> usize {
        self.live
            .iter()
            .filter(|a| group_of(a) == AgentState::NeedsInput)
            .count()
    }

    fn prompt(&self) -> Option<String> {
        match self.mode {
            Mode::Message => Some(format!("message> {}_", self.input)),
            Mode::Harness => Some("new agent: j/k pick  Enter launch  Esc cancel".to_string()),
            Mode::Browse => self.filter.prompt(),
        }
    }

    /// Draws inside `area`: the list (plus preview when wide enough) with its border.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let (list_area, preview_area) = self.split_area(area);
        self.render_list(out, list_area)?;
        match preview_area {
            Some(preview) => self.render_preview(out, preview),
            None => Ok(()),
        }
    }

    fn split_area(&self, area: Rect) -> (Rect, Option<Rect>) {
        if !self.preview || area.width < PREVIEW_MIN_WIDTH {
            return (area, None);
        }
        let left = area.width * 3 / 5;
        (
            Rect::new(area.x, area.y, left, area.height),
            Some(Rect::new(
                area.x + left,
                area.y,
                area.width - left,
                area.height,
            )),
        )
    }

    fn render_list<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let title = format!("Agents · {} need you", self.need_count());
        widgets::draw_box(out, area, &title, HINTS)?;
        let prompt = self.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        if self.mode == Mode::Harness {
            return self.render_harnesses(out, rows);
        }
        let lines = self.lines();
        if lines.is_empty() {
            return widgets::put_row(
                out,
                rows,
                0,
                &[Span::dim("no agents or resumable sessions")],
                false,
            );
        }
        let items: Vec<Vec<Span>> = lines
            .iter()
            .map(|l| self.line_spans(l, rows.width as usize))
            .collect();
        widgets::draw_items(
            out,
            rows,
            &items,
            cursor_line(&lines, self.selected()),
            &self.list,
        )
    }

    fn render_harnesses<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let items: Vec<Vec<Span>> = self
            .harnesses
            .iter()
            .map(|name| vec![Span::plain(format!("  {name}"))])
            .collect();
        widgets::draw_items(out, area, &items, self.harness_pick, &ListState::default())
    }

    fn line_spans(&self, line: &Line, width: usize) -> Vec<Span> {
        match line {
            Line::Header { title, glyph, tone } => {
                vec![Span::toned(format!("{glyph} {title}"), *tone)]
            }
            Line::Row(Slot::Live(i)) => self.live_spans(&self.live[*i], width),
            Line::Row(Slot::Resume(i)) => resume_spans(&self.resumable[*i], width),
        }
    }

    fn live_spans(&self, agent: &LiveAgent, width: usize) -> Vec<Span> {
        let node = &agent.node;
        let (glyph, tone) = state_mark(group_of(agent));
        let mark = if self.marks.contains(&agent_key(agent)) {
            '+'
        } else {
            ' '
        };
        let detail = node
            .agent
            .as_ref()
            .and_then(|a| a.detail.clone())
            .or_else(|| node.title.clone())
            .unwrap_or_default();
        let cols = Columns {
            lead: Span::toned(format!("{mark}{glyph} "), tone),
            name: node.name.clone(),
            harness: node
                .agent
                .as_ref()
                .map_or_else(String::new, |a| a.harness.clone()),
            reference: agent_key(agent),
            detail,
            age: age_label(now_ms().saturating_sub(since(agent))),
        };
        column_spans(cols, width)
    }

    fn render_preview<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let body = widgets::inner(area);
        let (title, lines) = self.preview_lines(body.height as usize);
        widgets::draw_box(out, area, &title, "")?;
        for (row, line) in lines.iter().enumerate() {
            widgets::put_row(out, body, row as u16, &[Span::plain(line.as_str())], false)?;
        }
        Ok(())
    }

    fn preview_lines(&self, height: usize) -> (String, Vec<String>) {
        match self.selected() {
            Some(Slot::Live(i)) => {
                let agent = &self.live[i];
                let text = self
                    .screens
                    .get(&agent_key(agent))
                    .map_or("", String::as_str);
                (
                    format!("Preview · {}", agent.node.name),
                    tail_lines(text, height),
                )
            }
            Some(Slot::Resume(i)) => {
                let session = &self.resumable[i];
                (
                    format!("Preview · {}", session.title),
                    transcript_preview(session, height),
                )
            }
            None => ("Preview".to_string(), Vec::new()),
        }
    }
}
