use std::collections::BTreeSet;

use mpx::agents::LiveAgent;
use mpx::protocol::{AgentInfo, AgentState, NodeInfo};
use mpx::tui::agent::{AgentKey, newly_waiting, waiting_elsewhere, waiting_keys};

fn node(name: &str, state: Option<AgentState>) -> NodeInfo {
    let mut node = NodeInfo::new(name.to_string(), true, "sh".to_string());
    node.agent = state.map(|state| AgentInfo {
        harness: "claude".to_string(),
        state,
        session_id: None,
        detail: None,
        since_ms: 1,
    });
    node
}

fn key(session: &str, name: &str) -> AgentKey {
    (session.to_string(), name.to_string())
}

fn set(keys: &[AgentKey]) -> BTreeSet<AgentKey> {
    keys.iter().cloned().collect()
}

fn live(session: &str, name: &str, state: AgentState) -> LiveAgent {
    LiveAgent {
        session: session.to_string(),
        endpoint: String::new(),
        node: node(name, Some(state)),
    }
}

#[test]
fn waiting_keys_count_only_agents_that_need_input_here_and_elsewhere() {
    let nodes = vec![
        node("a", Some(AgentState::NeedsInput)),
        node("b", Some(AgentState::Working)),
        node("c", None),
    ];
    let elsewhere = vec![key("other", "x")];
    assert_eq!(
        waiting_keys("main", &nodes, &elsewhere),
        set(&[key("main", "a"), key("other", "x")])
    );
}

#[test]
fn waiting_elsewhere_drops_this_session_and_idle_agents() {
    let found = vec![
        live("main", "here", AgentState::NeedsInput),
        live("work", "busy", AgentState::Working),
        live("work", "asks", AgentState::NeedsInput),
    ];
    assert_eq!(waiting_elsewhere(found, "main"), vec![key("work", "asks")]);
}

#[test]
fn newly_waiting_reports_only_agents_that_were_not_waiting_before() {
    let seen = set(&[key("main", "old")]);
    let now = set(&[key("main", "old"), key("main", "fresh")]);
    assert_eq!(
        newly_waiting(&seen, &now, "main", &[], true),
        vec![key("main", "fresh")]
    );
}

#[test]
fn newly_waiting_skips_agents_whose_pane_is_visible_in_this_window() {
    let now = set(&[key("main", "shown"), key("main", "hidden")]);
    let visible = vec!["shown".to_string()];
    assert_eq!(
        newly_waiting(&BTreeSet::new(), &now, "main", &visible, true),
        vec![key("main", "hidden")]
    );
}

#[test]
fn newly_waiting_always_reports_agents_in_other_sessions() {
    let now = set(&[key("other", "shown")]);
    let visible = vec!["shown".to_string()];
    assert_eq!(
        newly_waiting(&BTreeSet::new(), &now, "main", &visible, true),
        vec![key("other", "shown")]
    );
}

#[test]
fn newly_waiting_is_empty_when_notifications_are_disabled() {
    let now = set(&[key("main", "a")]);
    assert!(newly_waiting(&BTreeSet::new(), &now, "main", &[], false).is_empty());
}
