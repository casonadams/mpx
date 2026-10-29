//! The agents rail: shown only while agents run, a state dot per agent beside the panes; expanded
//! over the panes when focused or while stepping through agents, showing them live as the
//! highlight moves, within the session and across sessions.
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use mpx::client::{AgentReportArgs, Client};
use mpx::config::Config;
use mpx::protocol::AgentState;
use mpx::tui::app::Mode;
use mpx::tui::sidebar::{RAIL_COLS, SIDEBAR_COLS, SWITCH_DELAY};

use super::support::{ECHO, Harness, MAIN, SESSION, SIZE, connect, split, window};

async fn report(client: &mut Client, pane: &str, state: AgentState) {
    let args = AgentReportArgs {
        harness: Some("claude".to_string()),
        state: Some(state),
        ..AgentReportArgs::default()
    };
    client.agent_report(pane, args).await.expect("report agent");
}

/// `MAIN` and a second window `work` hold agents; `MAIN` is focused.
async fn two_agents() -> Harness {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    report(&mut seed, MAIN, AgentState::Working).await;
    report(&mut seed, "work", AgentState::NeedsInput).await;
    seed.select_window(0).await.expect("select main");
    h.app.refresh_sidebar().await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
    h
}

/// With a second session `other` whose one agent, `far`, is idle: focuses the rail and moves
/// the highlight to `far`.
async fn highlight_other_sessions_agent(h: &mut Harness) {
    h.sandbox.serve("other", &Config::default()).await;
    let mut other = connect(&h.sandbox.endpoint("other")).await;
    window(&mut other, "far", ECHO).await;
    report(&mut other, "far", AgentState::Idle).await;
    h.prefixed_char('e').await;
    h.key(KeyCode::Char('G')).await;
}

fn in_sidebar(h: &Harness) -> bool {
    matches!(h.app.mode, Mode::Sidebar)
}

/// The rail's cell on screen row `row`.
fn rail_cell(h: &Harness, row: usize) -> mpx::vt::Cell {
    *h.terminal.grid().get_cell(0, row).expect("rail cell")
}

#[tokio::test]
async fn the_rail_appears_with_the_first_agent() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h.chord(KeyCode::Char('j'), KeyModifiers::ALT).await;
    super::support::wait_for(&mut h.probe, MAIN, "\x1bj").await;
    h.prefixed_char('e').await;
    assert_eq!(h.app.live_message(), Some("no agents running"));
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, 0, "no agents, no rail");

    report(&mut h.probe, MAIN, AgentState::Idle).await;
    h.app.refresh_sidebar().await;
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, RAIL_COLS);
    assert_eq!(rail_cell(&h, 0).c, '○');
}

#[tokio::test]
async fn agents_get_a_rail_of_state_dots_with_the_shown_agent_reversed() {
    let h = two_agents().await;
    assert_eq!(h.rect_of(MAIN).x, RAIL_COLS);
    let (shown, other) = (rail_cell(&h, 0), rail_cell(&h, 1));
    assert_eq!((shown.c, other.c), ('●', '◆'), "working, then needs input");
    assert!(shown.style.reverse && !other.style.reverse);
}

#[tokio::test]
async fn prefix_e_expands_the_rail_over_the_panes_without_resizing_them() {
    let mut h = two_agents().await;
    let pane = h.rect_of(MAIN);
    h.prefixed_char('e').await;
    assert!(in_sidebar(&h));
    let text = h.frame().await;
    assert_eq!(h.rect_of(MAIN), pane, "the panes keep their size");
    let top = text.lines().next().unwrap_or("");
    assert!(top.starts_with(" Agents ◆1 ●1"), "{text}");
    assert!(text.contains("claude · main"), "{text}");
    assert!(text.contains("j/k move"), "{text}");

    h.ctrl('b').await;
    h.key(KeyCode::Esc).await;
    assert!(in_sidebar(&h), "a cancelled prefix returns to the rail");
    h.prefixed_char('e').await;
    assert!(h.mode_is_normal());
    let text = h.frame().await;
    assert!(!text.contains("Agents"), "collapsed again: {text}");
    assert_eq!(rail_cell(&h, 0).c, '●');

    h.prefixed_char('h').await;
    assert!(in_sidebar(&h), "select-pane left from the leftmost pane");
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn alt_j_and_alt_k_step_through_agents_without_opening_the_rail() {
    let mut h = two_agents().await;
    h.chord(KeyCode::Char('j'), KeyModifiers::ALT).await;
    assert!(h.mode_is_normal(), "keys keep going to the panes");
    let text = h.frame().await;
    assert_eq!(h.app.active_pane, "work");
    assert!(!text.contains(" Agents"), "the rail stays collapsed: {text}");
    assert!(rail_cell(&h, 1).style.reverse, "its dot shows the agent now in view");

    h.chord(KeyCode::Char('k'), KeyModifiers::ALT).await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
}

#[tokio::test]
async fn moving_the_highlight_shows_that_agent_and_enter_hands_keys_to_it() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    h.key(KeyCode::Char('j')).await;
    assert!(in_sidebar(&h));
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");

    h.key(KeyCode::Char('k')).await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);

    h.key(KeyCode::Enter).await;
    assert!(h.mode_is_normal());
    let text = h.frame().await;
    assert!(!text.contains("j/k move"), "{text}");
}

#[tokio::test]
async fn another_sessions_agent_shows_after_the_highlight_rests() {
    let mut h = two_agents().await;
    highlight_other_sessions_agent(&mut h).await;
    assert_eq!(h.app.session, SESSION, "switching waits for the highlight to rest");
    assert!(h.app.sidebar.due().is_some());

    tokio::time::sleep(SWITCH_DELAY + Duration::from_millis(20)).await;
    h.app.on_due().await;
    assert_eq!(h.app.session, "other");
    assert!(in_sidebar(&h), "the rail keeps focus across the switch");
    h.frame().await;
    assert_eq!(h.app.active_pane, "far");
    assert_eq!(h.rect_of("far").x, RAIL_COLS);
}

#[tokio::test]
async fn enter_switches_sessions_at_once_and_focuses_the_agent() {
    let mut h = two_agents().await;
    highlight_other_sessions_agent(&mut h).await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.app.session, "other");
    assert!(h.mode_is_normal());
    assert!(h.app.sidebar.due().is_none());
}

#[tokio::test]
async fn clicking_a_dot_shows_its_agent_and_clicking_a_pane_returns_focus() {
    let mut h = two_agents().await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 1).await;
    assert!(in_sidebar(&h));
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");

    h.mouse(MouseEventKind::Down(MouseButton::Left), SIDEBAR_COLS + 5, 3)
        .await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn narrow_screens_keep_the_rail_out_of_the_pane_area() {
    let mut h = two_agents().await;
    h.app.fixed_size = Some((RAIL_COLS + 30, SIZE.1));
    h.frame().await;
    h.prefixed_char('e').await;
    assert!(h.mode_is_normal(), "a rail with no room takes no focus");
    assert_eq!(h.app.live_message(), Some("screen too narrow for the agents rail"));
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, 0);
}

#[tokio::test]
async fn shrinking_the_screen_hides_the_rail_and_returns_focus_to_the_panes() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    h.frame().await;
    assert!(in_sidebar(&h));
    h.app.fixed_size = Some((RAIL_COLS + 30, SIZE.1));
    h.frame().await;
    assert!(h.mode_is_normal());
    assert_eq!(h.rect_of(MAIN).x, 0);
}

#[tokio::test]
async fn moving_right_out_of_the_rail_keeps_the_shown_agent_focused() {
    let mut h = two_agents().await;
    split(&mut h.probe, "work", "beside", mpx::layout::Direction::Horizontal).await;
    h.prefixed_char('e').await;
    h.key(KeyCode::Char('j')).await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");
    h.prefixed_char('l').await;
    h.frame().await;
    assert!(h.mode_is_normal() && h.app.expanded_width() == 0, "the rail collapses");
    assert_eq!(h.app.active_pane, "work", "not the pane beside it");
}
