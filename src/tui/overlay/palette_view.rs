//! Unified command palette: tabbed modal with instant fuzzy search across
//! commands, agents, sessions/panes, and help.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::agents::{LiveAgent, ResumableSession, list_live, scan_resumable};
use crate::layout::Rect;
use crate::protocol::AgentState;
use crate::tui::command::{Command, PaletteTab};
use crate::tui::overlay::agents_view::agent_key;
use crate::tui::overlay::nav::{ListState, fuzzy_match, has_ctrl};
use crate::tui::overlay::tree_view::{TreePane, TreeSession, TreeWindow, load_session};
use crate::tui::overlay::widgets::{
    self, Span, Tone, age_label, now_ms, put_row, state_mark,
};
use crate::tui::overlay::{HelpEntry, OverlayAction, OverlayCtx};
use crate::tui::prompt::PromptState;

const HINTS: &str = "◄ ►/Tab: tabs  ▲ ▼/C-n/p: move  Enter: run  Esc: close";

struct BuiltinEntry {
    cmd: &'static str,
    title: &'static str,
    shortcut: &'static str,
    desc: &'static str,
    command: Command,
}

static BUILTIN_ENTRIES: std::sync::LazyLock<Vec<BuiltinEntry>> = std::sync::LazyLock::new(|| {
    BUILTIN_COMMANDS
        .iter()
        .filter_map(|&(cmd, title, shortcut, desc)| {
            crate::tui::command::parse(cmd).ok().map(|command| BuiltinEntry {
                cmd,
                title,
                shortcut,
                desc,
                command,
            })
        })
        .collect()
});

pub const BUILTIN_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("new-window", "New Window", "c", "Create a new window in the active session"),
    ("split-window", "Split Horizontal", "\" or -", "Split the active pane horizontally"),
    ("split-window -h", "Split Vertical", "% or |", "Split the active pane vertically"),
    ("next-window", "Next Window", "n", "Select the next window"),
    ("previous-window", "Previous Window", "p", "Select the previous window"),
    ("last-window", "Last Window", "^", "Select the last active window"),
    ("select-pane -L", "Focus Left", "h / Left", "Move focus to the left pane"),
    ("select-pane -D", "Focus Down", "j / Down", "Move focus to the pane below"),
    ("select-pane -U", "Focus Up", "k / Up", "Move focus to the pane above"),
    ("select-pane -R", "Focus Right", "l / Right", "Move focus to the right pane"),
    ("next-pane", "Next Pane", "o / Tab", "Move focus to the next pane"),
    ("last-pane", "Last Pane", ";", "Move focus to the last active pane"),
    ("resize-pane -Z", "Toggle Zoom", "z", "Zoom or unzoom the active pane"),
    ("kill-pane", "Kill Pane", "x", "Close the active pane"),
    ("kill-window", "Kill Window", "&", "Close the active window and all its panes"),
    ("select-pane -m", "Mark Pane", "m", "Toggle the marked flag on the active pane"),
    ("select-pane -M", "Clear Mark", "M", "Clear the marked pane"),
    ("break-pane", "Break Pane", "!", "Break the active pane into a new window"),
    ("swap-pane", "Swap With Marked", "", "Swap active pane with the marked pane"),
    ("swap-pane -U", "Swap Pane Up", "{", "Swap active pane with the previous one"),
    ("swap-pane -D", "Swap Pane Down", "}", "Swap active pane with the next one"),
    ("next-layout", "Next Layout", "Space", "Cycle to the next layout arrangement"),
    ("select-layout -E", "Spread Layout Evenly", "=", "Evenly spread out all pane dimensions"),
    ("copy-mode", "Copy / Scrollback Mode", "[", "Enter scrollback history and copy mode"),
    ("detach-client", "Detach Session", "d / q", "Detach the current client from the session"),
    ("next-session", "Next Session", ")", "Switch to the next active session"),
    ("prev-session", "Previous Session", "(", "Switch to the previous active session"),
    ("palette agents", "Agent Manager", "s", "Open the agent sessions palette tab"),
    ("palette sessions", "Window / Session Tree", "w", "Open the sessions and panes palette tab"),
    ("palette help", "Keymap Help", "?", "Open the key bindings help palette tab"),
    ("sidebar", "Toggle Sidebar", ".", "Toggle the vertical agents drawer"),
    ("agents", "Agent Manager (Deep)", "", "Open the classic agent manager overlay with preview"),
    ("choose-tree", "Window Tree (Deep)", "", "Open the classic sessions, windows, and panes tree"),
    ("list-keys", "Keymap Help (Deep)", "", "Open the classic key bindings cheat sheet"),
    ("command-prompt", "Command Prompt", ":", "Open the command prompt"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub shortcut: Option<String>,
    pub badge: Option<Span>,
    pub actions: Vec<Command>,
}

pub struct PaletteView {
    pub tab: PaletteTab,
    pub prompt: PromptState,
    pub list: ListState,
    pub help_entries: Vec<HelpEntry>,
    pub current_session: String,
    pub live_agents: Vec<LiveAgent>,
    pub resumable: Vec<ResumableSession>,
    pub sessions: Vec<TreeSession>,
    pub cached_items: Vec<PaletteItem>,
    pub initial_loaded: bool,
}

impl PaletteTab {
    pub fn next(self) -> Self {
        match self {
            Self::Commands => Self::Agents,
            Self::Agents => Self::Sessions,
            Self::Sessions => Self::Help,
            Self::Help => Self::Commands,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Commands => Self::Help,
            Self::Agents => Self::Commands,
            Self::Sessions => Self::Agents,
            Self::Help => Self::Sessions,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Agents => "Agents",
            Self::Sessions => "Sessions & Panes",
            Self::Help => "Help",
        }
    }
}

impl PaletteView {
    pub fn new(tab: PaletteTab, help_entries: Vec<HelpEntry>, current_session: &str) -> Self {
        let mut view = Self {
            tab,
            prompt: PromptState::default(),
            list: ListState::default(),
            help_entries,
            current_session: current_session.to_string(),
            live_agents: Vec::new(),
            resumable: Vec::new(),
            sessions: Vec::new(),
            cached_items: Vec::new(),
            initial_loaded: false,
        };
        view.rebuild_items();
        view
    }

    pub fn rebuild_items(&mut self) {
        self.cached_items = self.compute_items();
    }

    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        if !self.initial_loaded {
            self.initial_loaded = true;
            self.live_agents = list_live(ctx.socket_dir).await;
            self.resumable = scan_resumable(
                ctx.registry,
                ctx.cwd.map(std::path::PathBuf::from).as_deref(),
                50,
            );
            let mut sessions = Vec::new();
            for endpoint in crate::ipc::list_sessions(ctx.socket_dir) {
                if let Some(session) = load_session(&endpoint.endpoint).await {
                    sessions.push(session);
                }
            }
            sessions.sort_by_key(|s| s.name != self.current_session);
            self.sessions = sessions;
            self.rebuild_items();
            return;
        }

        match self.tab {
            PaletteTab::Commands | PaletteTab::Help => {}
            PaletteTab::Agents => {
                self.live_agents = list_live(ctx.socket_dir).await;
                self.resumable = scan_resumable(
                    ctx.registry,
                    ctx.cwd.map(std::path::PathBuf::from).as_deref(),
                    50,
                );
                self.rebuild_items();
            }
            PaletteTab::Sessions => {
                let mut sessions = Vec::new();
                for endpoint in crate::ipc::list_sessions(ctx.socket_dir) {
                    if let Some(session) = load_session(&endpoint.endpoint).await {
                        sessions.push(session);
                    }
                }
                sessions.sort_by_key(|s| s.name != self.current_session);
                self.sessions = sessions;
                self.rebuild_items();
            }
        }
    }

    pub async fn follow_selection(&mut self) {}

    pub fn current_items(&self) -> Vec<PaletteItem> {
        if self.cached_items.is_empty() && self.prompt.text().is_empty() {
            self.compute_items()
        } else {
            self.cached_items.clone()
        }
    }

    fn compute_items(&self) -> Vec<PaletteItem> {
        let query = self.prompt.text();
        match self.tab {
            PaletteTab::Commands => self.command_items(&query),
            PaletteTab::Agents => self.agent_items(&query),
            PaletteTab::Sessions => self.session_items(&query),
            PaletteTab::Help => self.help_items(&query),
        }
    }

    fn command_items(&self, query: &str) -> Vec<PaletteItem> {
        let mut items = Vec::new();

        // If user typed an explicit raw command (e.g. starting with ':' or arbitrary valid command)
        let trimmed = query.trim_start_matches(':').trim();
        if !trimmed.is_empty()
            && let Ok(cmd) = crate::tui::command::parse(trimmed)
            && !BUILTIN_COMMANDS.iter().any(|(cmd, ..)| *cmd == trimmed)
        {
            items.push(PaletteItem {
                title: format!("Run: {trimmed}"),
                subtitle: format!("Execute '{trimmed}'"),
                shortcut: None,
                badge: Some(Span::bold("EXEC", Tone::Accent)),
                actions: vec![cmd],
            });
        }

        for b in BUILTIN_ENTRIES.iter() {
            if query.is_empty()
                || fuzzy_match(query, b.title)
                || fuzzy_match(query, b.cmd)
                || fuzzy_match(query, b.desc)
                || fuzzy_match(query, b.shortcut)
            {
                items.push(PaletteItem {
                    title: b.title.to_string(),
                    subtitle: b.desc.to_string(),
                    shortcut: Some(b.shortcut.to_string()),
                    badge: None,
                    actions: vec![b.command.clone()],
                });
            }
        }

        items
    }

    fn agent_items(&self, query: &str) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        self.collect_live_agent_items(query, &mut items);
        self.collect_resumable_items(query, &mut items);
        items
    }

    fn collect_live_agent_items(&self, query: &str, items: &mut Vec<PaletteItem>) {
        for agent in &self.live_agents {
            let name = &agent.node.name;
            let harness = agent
                .node
                .agent
                .as_ref()
                .map_or("agent", |a| a.harness.as_str());
            let detail = agent
                .node
                .agent
                .as_ref()
                .and_then(|a| a.detail.as_deref())
                .unwrap_or("");
            let state = agent
                .node
                .agent
                .as_ref()
                .map_or(AgentState::Idle, |a| a.state);

            if query.is_empty()
                || fuzzy_match(query, name)
                || fuzzy_match(query, harness)
                || fuzzy_match(query, &agent.session)
                || fuzzy_match(query, detail)
            {
                let (glyph, tone) = state_mark(state);
                let target = agent_key(agent);
                let badge_label = match state {
                    AgentState::NeedsInput => "needs input",
                    AgentState::Working => "working",
                    AgentState::Idle => "idle",
                    AgentState::Exited => "exited",
                };
                items.push(PaletteItem {
                    title: name.clone(),
                    subtitle: format!(
                        "{harness} · {}:%{} · {}",
                        agent.session, agent.node.id, detail
                    ),
                    shortcut: Some(format!("{}:%{}", agent.session, agent.node.id)),
                    badge: Some(Span::toned(format!("{glyph} {badge_label}"), tone)),
                    actions: vec![Command::SelectPane { target }],
                });
            }
        }
    }

    fn collect_resumable_items(&self, query: &str, items: &mut Vec<PaletteItem>) {
        for res in &self.resumable {
            let title = &res.title;
            let harness = &res.harness;
            let id = &res.id;

            if query.is_empty()
                || fuzzy_match(query, title)
                || fuzzy_match(query, harness)
                || fuzzy_match(query, id)
            {
                let age = age_label(now_ms().saturating_sub(res.modified_ms));
                items.push(PaletteItem {
                    title: title.clone(),
                    subtitle: format!("{harness} · {age} ago · id {id}"),
                    shortcut: Some(id.chars().take(8).collect()),
                    badge: Some(Span::toned("◌ resume", Tone::Accent)),
                    actions: vec![Command::LaunchAgent {
                        harness: harness.clone(),
                        resume: Some(id.clone()),
                        cwd: res.cwd.clone(),
                        split: false,
                    }],
                });
            }
        }
    }

    fn session_items(&self, query: &str) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        for session in &self.sessions {
            self.collect_session_items(session, query, &mut items);
        }
        items
    }

    fn collect_session_items(
        &self,
        session: &TreeSession,
        query: &str,
        items: &mut Vec<PaletteItem>,
    ) {
        for window in &session.windows {
            self.collect_window_item(session, window, query, items);
        }
    }

    fn collect_window_item(
        &self,
        session: &TreeSession,
        window: &TreeWindow,
        query: &str,
        items: &mut Vec<PaletteItem>,
    ) {
        let win_title = format!("{}: {}:{}", session.name, window.index, window.name);
        if query.is_empty()
            || fuzzy_match(query, &session.name)
            || fuzzy_match(query, &window.name)
            || fuzzy_match(query, &win_title)
        {
            let actions = if session.name != self.current_session {
                vec![
                    Command::SwitchSession {
                        name: session.name.clone(),
                    },
                    Command::SelectWindow {
                        index: window.index,
                    },
                ]
            } else {
                vec![Command::SelectWindow {
                    index: window.index,
                }]
            };

            let badge = if window.active && session.name == self.current_session {
                Some(Span::toned("current", Tone::Active))
            } else if window.active {
                Some(Span::dim("active"))
            } else {
                None
            };

            let count = window.panes.len();
            items.push(PaletteItem {
                title: win_title,
                subtitle: format!("{count} pane{}", if count == 1 { "" } else { "s" }),
                shortcut: Some(format!("{}:{}", session.name, window.index)),
                badge,
                actions,
            });
        }

        if window.panes.len() > 1 {
            for pane in &window.panes {
                collect_pane_row(&session.name, &window.name, pane, query, items);
            }
        }
    }
}

fn collect_pane_row(
    sess: &str,
    win: &str,
    pane: &TreePane,
    query: &str,
    items: &mut Vec<PaletteItem>,
) {
    let pane_title = format!("{sess}:{win}.{} (%{})", pane.name, pane.id);
    if query.is_empty()
        || fuzzy_match(query, &pane.name)
        || fuzzy_match(query, &pane.program)
        || fuzzy_match(query, &pane_title)
    {
        let target = format!("{sess}:{}", pane.name);
        items.push(PaletteItem {
            title: format!("  └ {}", pane.name),
            subtitle: pane.program.clone(),
            shortcut: Some(format!("%{}", pane.id)),
            badge: None,
            actions: vec![Command::SelectPane { target }],
        });
    }
}

impl PaletteView {

    fn help_items(&self, query: &str) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        for entry in &self.help_entries {
            if query.is_empty()
                || fuzzy_match(query, &entry.key)
                || fuzzy_match(query, &entry.command)
            {
                let actions = crate::tui::command::parse(&entry.command)
                    .map_or_else(|_| Vec::new(), |cmd| vec![cmd]);
                items.push(PaletteItem {
                    title: entry.key.clone(),
                    subtitle: entry.command.clone(),
                    shortcut: None,
                    badge: None,
                    actions,
                });
            }
        }
        items
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        if let Some(action) = self.handle_nav_key(key) {
            return action;
        }

        if self.handle_edit_key(key) {
            self.list.cursor = 0;
            self.rebuild_items();
        }

        OverlayAction::None
    }

    fn handle_nav_key(&mut self, key: KeyEvent) -> Option<OverlayAction> {
        let is_alt = key.modifiers.contains(KeyModifiers::ALT);
        if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && has_ctrl(key)) {
            return Some(OverlayAction::Close);
        }
        if key.code == KeyCode::Enter {
            let action = self
                .cached_items
                .get(self.list.cursor)
                .filter(|item| !item.actions.is_empty())
                .map_or(OverlayAction::Close, |item| {
                    OverlayAction::Run(item.actions.clone())
                });
            return Some(action);
        }
        self.handle_direction_key(key, is_alt)
    }

    fn handle_direction_key(&mut self, key: KeyEvent, is_alt: bool) -> Option<OverlayAction> {
        let len = self.cached_items.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('∆') => self.step_cursor(1, len),
            KeyCode::Char('n') if has_ctrl(key) => self.step_cursor(1, len),
            KeyCode::Char('j') if is_alt => self.step_cursor(1, len),

            KeyCode::Up | KeyCode::Char('˚') => self.step_cursor(-1, len),
            KeyCode::Char('p') if has_ctrl(key) => self.step_cursor(-1, len),
            KeyCode::Char('k') if is_alt => self.step_cursor(-1, len),

            KeyCode::Tab | KeyCode::Right => self.switch_tab(true),
            KeyCode::Char('l') if is_alt => self.switch_tab(true),

            KeyCode::BackTab | KeyCode::Left => self.switch_tab(false),
            KeyCode::Char('h') if is_alt => self.switch_tab(false),

            _ => None,
        }
    }

    fn step_cursor(&mut self, dir: i32, len: usize) -> Option<OverlayAction> {
        if dir > 0 {
            self.list.cursor = (self.list.cursor + 1).min(len.saturating_sub(1));
        } else {
            self.list.cursor = self.list.cursor.saturating_sub(1);
        }
        Some(OverlayAction::None)
    }

    fn switch_tab(&mut self, next: bool) -> Option<OverlayAction> {
        self.tab = if next {
            self.tab.next()
        } else {
            self.tab.prev()
        };
        self.list.cursor = 0;
        self.rebuild_items();
        Some(OverlayAction::None)
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Backspace => {
                self.prompt.backspace();
                true
            }
            KeyCode::Char('w') if has_ctrl(key) => {
                self.prompt.delete_word();
                true
            }
            KeyCode::Char('u') if has_ctrl(key) => {
                self.prompt.clear();
                true
            }
            KeyCode::Char(c)
                if !has_ctrl(key) && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.prompt.insert(c);
                true
            }
            _ => false,
        }
    }

    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let w = area.width.min(86);
        let h = area.height.min(26);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let box_area = Rect::new(x, y, w, h);

        widgets::draw_panel(out, box_area, "mpx palette", HINTS)?;
        let body = widgets::inner(box_area);
        if body.height < 4 || body.width < 10 {
            return Ok(());
        }

        self.render_tabs(out, body)?;
        self.render_search(out, body)?;
        let divider = "─".repeat(body.width as usize);
        put_row(out, body, 2, &[Span::dim(divider)], false)?;
        self.render_items(out, body)
    }

    fn render_tabs<W: Write>(&self, out: &mut W, body: Rect) -> io::Result<()> {
        let mut tab_spans = Vec::new();
        for t in [
            PaletteTab::Commands,
            PaletteTab::Agents,
            PaletteTab::Sessions,
            PaletteTab::Help,
        ] {
            let count_label = match t {
                PaletteTab::Agents if !self.live_agents.is_empty() => {
                    format!(" ({})", self.live_agents.len())
                }
                PaletteTab::Sessions if !self.sessions.is_empty() => {
                    format!(" ({})", self.sessions.len())
                }
                _ => String::new(),
            };
            let label = format!("{}{count_label}", t.label());
            if t == self.tab {
                tab_spans.push(Span::bold(format!("[ {label} ]"), Tone::Accent));
            } else {
                tab_spans.push(Span::dim(format!("  {label}  ")));
            }
            tab_spans.push(Span::plain(" "));
        }
        put_row(out, body, 0, &tab_spans, false)
    }

    fn render_search<W: Write>(&self, out: &mut W, body: Rect) -> io::Result<()> {
        let search_text = self.prompt.text();
        let prompt_spans = vec![
            Span::bold(" > ", Tone::Accent),
            Span::plain(&search_text),
            Span::dim("_"),
        ];
        put_row(out, body, 1, &prompt_spans, false)
    }

    fn render_items<W: Write>(&self, out: &mut W, body: Rect) -> io::Result<()> {
        let visible_h = body.height.saturating_sub(3) as usize;
        let items = if self.cached_items.is_empty() && self.prompt.text().is_empty() {
            self.compute_items()
        } else {
            self.cached_items.clone()
        };
        let range = self.list.window(self.list.cursor, items.len(), visible_h);

        if items.is_empty() {
            return put_row(
                out,
                body,
                3,
                &[Span::dim("  No matching items found")],
                false,
            );
        }

        for (i, item) in items[range.clone()].iter().enumerate() {
            let selected = (range.start + i) == self.list.cursor;
            let row_spans = self.item_spans(item, selected);
            put_row(out, body, 3 + i as u16, &row_spans, selected)?;
        }
        Ok(())
    }

    fn item_spans(&self, item: &PaletteItem, selected: bool) -> Vec<Span> {
        let mut row_spans = Vec::new();
        if selected {
            row_spans.push(Span::bold("▶ ", Tone::Accent));
        } else {
            row_spans.push(Span::plain("  "));
        }

        if let Some(badge) = &item.badge {
            row_spans.push(badge.clone());
            row_spans.push(Span::plain(" "));
        }

        if selected {
            row_spans.push(Span::bold(&item.title, Tone::Accent));
        } else {
            row_spans.push(Span::plain(&item.title));
        }

        if let Some(sc) = &item.shortcut {
            row_spans.push(Span::plain("  "));
            row_spans.push(Span::dim(format!("[{sc}]")));
        }

        if !item.subtitle.is_empty() {
            row_spans.push(Span::plain("  ·  "));
            row_spans.push(Span::dim(&item.subtitle));
        }

        row_spans
    }
}
