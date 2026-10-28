//! The agents sidebar: toggling it, the pane area it leaves, and showing agents live as the
//! highlight moves, within the session and across sessions.
use std::time::Duration;

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::client::{AgentReportArgs, Client};
use mpx::config::Config;
use mpx::protocol::AgentState;
use mpx::tui::app::Mode;
use mpx::tui::sidebar::{SIDEBAR_COLS, SWITCH_DELAY};

use super::support::{ECHO, Harness, MAIN, SESSION, SIZE, connect, window};

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
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
    h
}

fn in_sidebar(h: &Harness) -> bool {
    matches!(h.app.mode, Mode::Sidebar)
}

#[tokio::test]
async fn toggling_shows_a_focused_sidebar_left_of_the_panes_and_hides_it() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    assert!(in_sidebar(&h));
    let text = h.frame().await;
    let pane = h.rect_of(MAIN);
    assert_eq!((pane.x, pane.width), (SIDEBAR_COLS, SIZE.0 - SIDEBAR_COLS));
    let top = text.lines().next().unwrap_or("");
    assert!(top.starts_with(" Agents ◆1 ●1"), "{text}");
    assert!(text.contains("claude · main"), "{text}");
    assert!(text.contains("j/k move"), "{text}");

    h.prefixed_char('e').await;
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, 0);
    assert!(h.mode_is_normal());
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
    h.sandbox.serve("other", &Config::default()).await;
    let mut other = connect(&h.sandbox.endpoint("other")).await;
    window(&mut other, "far", ECHO).await;
    report(&mut other, "far", AgentState::Idle).await;

    h.prefixed_char('e').await;
    h.key(KeyCode::Char('G')).await;
    assert_eq!(h.app.session, SESSION, "switching waits for the highlight to rest");
    assert!(h.app.sidebar.due().is_some());

    tokio::time::sleep(SWITCH_DELAY + Duration::from_millis(20)).await;
    h.app.on_due().await;
    assert_eq!(h.app.session, "other");
    assert!(in_sidebar(&h), "the sidebar keeps focus across the switch");
    h.frame().await;
    assert_eq!(h.app.active_pane, "far");
    assert_eq!(h.rect_of("far").x, SIDEBAR_COLS);
}

#[tokio::test]
async fn enter_switches_sessions_at_once_and_focuses_the_agent() {
    let mut h = two_agents().await;
    h.sandbox.serve("other", &Config::default()).await;
    let mut other = connect(&h.sandbox.endpoint("other")).await;
    window(&mut other, "far", ECHO).await;
    report(&mut other, "far", AgentState::Idle).await;

    h.prefixed_char('e').await;
    h.key(KeyCode::Char('G')).await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.app.session, "other");
    assert!(h.mode_is_normal());
    assert!(h.app.sidebar.due().is_none());
}

#[tokio::test]
async fn select_pane_left_from_the_leftmost_pane_focuses_the_sidebar() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());
    h.prefixed_char('h').await;
    assert!(in_sidebar(&h));
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, 0);
}

#[tokio::test]
async fn clicking_an_agent_row_shows_it_and_clicking_a_pane_returns_focus() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    h.key(KeyCode::Esc).await;
    let text = h.frame().await;
    let row = text
        .lines()
        .position(|l| l.contains("claude · work"))
        .expect("work agent listed") as u16;

    h.mouse(MouseEventKind::Down(MouseButton::Left), 3, row).await;
    assert!(in_sidebar(&h));
    h.frame().await;
    assert_eq!(h.app.active_pane, "work");

    h.mouse(MouseEventKind::Down(MouseButton::Left), SIDEBAR_COLS + 5, 3)
        .await;
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn narrow_screens_keep_the_sidebar_out_of_the_pane_area() {
    let mut h = two_agents().await;
    h.app.fixed_size = Some((SIDEBAR_COLS + 20, SIZE.1));
    h.frame().await;
    h.prefixed_char('e').await;
    assert!(h.mode_is_normal(), "a sidebar with no room takes no focus");
    assert_eq!(h.app.live_message(), Some("screen too narrow for the sidebar"));
    h.frame().await;
    assert_eq!(h.rect_of(MAIN).x, 0);
}

#[tokio::test]
async fn shrinking_the_screen_hides_the_sidebar_and_returns_focus_to_the_panes() {
    let mut h = two_agents().await;
    h.prefixed_char('e').await;
    h.frame().await;
    assert!(in_sidebar(&h));
    h.app.fixed_size = Some((SIDEBAR_COLS + 20, SIZE.1));
    h.frame().await;
    assert!(h.mode_is_normal());
    assert_eq!(h.rect_of(MAIN).x, 0);
}
