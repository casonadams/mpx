use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::agents::LiveAgent;
use mpx::protocol::{AgentInfo, AgentState, NodeInfo, NodeState};
use mpx::tui::sidebar::{
    SIDEBAR_COLS, Sidebar, View, body_lines, headline, ordered, render, scroll_top, width,
};
use mpx::vt::Color;

const NOW: u64 = 10_000_000;

fn agent(session: &str, name: &str, id: u32, state: AgentState, title: Option<&str>) -> LiveAgent {
    let mut node = NodeInfo::new(name.to_string(), true, "claude".to_string());
    node.id = id;
    node.title = title.map(str::to_string);
    node.agent = Some(AgentInfo {
        harness: "claude".to_string(),
        state,
        session_id: None,
        detail: None,
        since_ms: NOW - 59_000,
        focus: Default::default(),
    });
    LiveAgent {
        session: session.to_string(),
        endpoint: String::new(),
        node,
    }
}

/// `main` (attached) has a waiting and a working agent; `work` has an exited one.
fn fleet() -> Vec<LiveAgent> {
    let mut gone = agent("work", "old", 1, AgentState::Idle, None);
    gone.node.state = NodeState::Exited;
    ordered(
        vec![
            gone,
            agent("main", "zsh-3", 3, AgentState::Working, Some("⠂ Billing branch")),
            agent("main", "zsh-0", 0, AgentState::NeedsInput, Some("✳ Session window UI")),
        ],
        "main",
    )
}

fn key(name: &str) -> (String, String) {
    ("main".to_string(), name.to_string())
}

fn view<'a>(
    agents: &'a [LiveAgent],
    highlight: Option<&'a (String, String)>,
    focused: bool,
) -> View<'a> {
    View {
        agents,
        current: "main",
        highlight,
        focused,
        now_ms: NOW,
    }
}

fn texts(agents: &[LiveAgent], highlight: Option<&(String, String)>) -> Vec<String> {
    body_lines(&view(agents, highlight, true), 33)
        .iter()
        .map(|l| l.text().trim_end().to_string())
        .collect()
}

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

#[test]
fn sidebar_only_takes_columns_when_shown_and_the_panes_keep_room() {
    assert_eq!(width(false, 200), 0);
    assert_eq!(width(true, 200), SIDEBAR_COLS);
    assert_eq!(width(true, SIDEBAR_COLS + 39), 0);
    assert_eq!(width(true, SIDEBAR_COLS + 40), SIDEBAR_COLS);
}

#[test]
fn headline_drops_leading_status_glyphs_and_falls_back_to_the_pane_name() {
    let agents = fleet();
    assert_eq!(headline(&agents[0].node), "Session window UI");
    assert_eq!(headline(&agents[1].node), "Billing branch");
    assert_eq!(headline(&agents[2].node), "old");
}

#[test]
fn agents_list_the_attached_session_first_then_by_session_and_pane_id() {
    let names: Vec<String> = fleet().into_iter().map(|a| a.node.name).collect();
    assert_eq!(names, vec!["zsh-0", "zsh-3", "old"]);
}

#[test]
fn body_groups_agents_under_session_headers_with_a_meta_line_each() {
    let lines = texts(&fleet(), None);
    assert_eq!(lines.len(), 9, "{lines:#?}");
    assert!(lines[0].starts_with(" main") && lines[0].contains("◆1 ●1"), "{lines:#?}");
    assert!(lines[1].contains("◆ Session window UI"), "{lines:#?}");
    assert!(lines[2].contains("claude · zsh-0") && lines[2].ends_with("59s"), "{lines:#?}");
    assert!(lines[3].contains("● Billing branch"), "{lines:#?}");
    assert_eq!(lines[5], "");
    assert!(lines[6].starts_with(" work") && lines[6].contains("✕1"), "{lines:#?}");
    assert!(lines[7].contains("✕ old"), "{lines:#?}");
}

#[test]
fn meta_line_shows_the_agents_message_instead_of_its_harness() {
    let mut agents = fleet();
    if let Some(info) = agents[0].node.agent.as_mut() {
        info.detail = Some("approve: cargo test".to_string());
    }
    let lines = texts(&agents, None);
    assert!(lines[2].trim_start().starts_with("approve: cargo test"), "{lines:#?}");
    assert!(!lines[2].contains("claude"), "{lines:#?}");
}

#[test]
fn body_marks_the_highlighted_agent_with_a_bar() {
    let lit = key("zsh-3");
    let lines = texts(&fleet(), Some(&lit));
    assert!(lines[3].starts_with('▌'), "{lines:#?}");
    assert!(!lines[1].starts_with('▌'), "{lines:#?}");
}

#[test]
fn body_says_so_when_no_agent_runs() {
    assert_eq!(texts(&[], None), vec!["  no agents running"]);
}

#[test]
fn render_fills_the_column_with_a_border_title_and_focus_hints() {
    let agents = fleet();
    let drawn = render(&view(&agents, None, true), 0, (34, 12), Color::Green);
    assert_eq!(drawn.rows.len(), 12);
    assert!(drawn.rows.iter().all(|r| r.len() == 34));
    assert!(drawn.rows.iter().all(|r| r[33].c == '│' && r[33].style.fg == Some(Color::Green)));
    let row = |i: usize| drawn.rows[i].iter().map(|c| c.c).collect::<String>();
    assert!(row(0).starts_with(" Agents ◆1 ●1 ✕1"), "{}", row(0));
    assert!(row(11).contains("j/k move"), "{}", row(11));
    assert_eq!(drawn.agents[2], Some(key("zsh-0")));
    assert_eq!(drawn.agents[3], Some(key("zsh-0")));
    assert_eq!(drawn.agents[1], None);

    let unfocused = render(&view(&agents, None, false), 0, (34, 12), Color::Green);
    let last: String = unfocused.rows[11].iter().map(|c| c.c).collect();
    assert!(!last.contains("j/k"), "{last}");
}

#[test]
fn render_scrolls_to_keep_the_highlighted_agent_in_view() {
    let agents = fleet();
    let lit = ("work".to_string(), "old".to_string());
    let drawn = render(&view(&agents, Some(&lit), true), 0, (34, 5), Color::Green);
    assert_eq!(drawn.top, 6);
    assert_eq!(drawn.agents[1], None, "the session header stays above its agent");
    assert_eq!(drawn.agents[2], Some(lit.clone()));
    assert_eq!(drawn.agents[3], Some(lit));
}

#[test]
fn scroll_top_moves_only_as_far_as_needed() {
    assert_eq!(scroll_top(0, 2, 10, 20), 0);
    assert_eq!(scroll_top(0, 9, 10, 20), 1);
    assert_eq!(scroll_top(5, 3, 10, 20), 3);
    assert_eq!(scroll_top(15, 12, 10, 20), 10);
    assert_eq!(scroll_top(0, 0, 0, 4), 0);
}

#[test]
fn select_from_prefers_the_shown_agent_then_the_kept_selection_then_the_first() {
    let mut bar = Sidebar::default();
    bar.agents = fleet();
    bar.select_from(Some(key("zsh-3")));
    assert_eq!(bar.selected, Some(key("zsh-3")));
    bar.select_from(Some(key("plain-shell")));
    assert_eq!(bar.selected, Some(key("zsh-3")));
    bar.selected = None;
    bar.select_from(None);
    assert_eq!(bar.selected, Some(key("zsh-0")));
}

#[test]
fn motion_walks_agents_across_sessions_and_reports_moves() {
    let mut bar = Sidebar::default();
    bar.agents = fleet();
    bar.select_from(None);
    assert_eq!(bar.motion(press('j')), Some(true));
    assert_eq!(bar.selected, Some(key("zsh-3")));
    assert_eq!(bar.motion(press('G')), Some(true));
    assert_eq!(bar.selected, Some(("work".to_string(), "old".to_string())));
    assert_eq!(bar.motion(press('j')), Some(false));
    assert_eq!(bar.motion(press('x')), None);
}
