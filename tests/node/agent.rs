use mpx::node::agent::{AgentTracker, Report, unhooked_state};
use mpx::protocol::AgentState;

fn report(state: Option<AgentState>) -> Report {
    Report {
        harness: Some("claude".to_string()),
        state,
        ..Report::default()
    }
}

fn state_of(t: &AgentTracker) -> Option<AgentState> {
    t.info().map(|i| i.state)
}

#[test]
fn unhooked_output_recency_decides_working_or_idle() {
    assert_eq!(unhooked_state(None, 10, 100), AgentState::Working);
    assert_eq!(unhooked_state(None, 100, 100), AgentState::Idle);
    assert_eq!(
        unhooked_state(Some(AgentState::Idle), 5, 100),
        AgentState::Working
    );
}

#[test]
fn needs_input_and_exited_are_sticky_without_hooks() {
    assert_eq!(
        unhooked_state(Some(AgentState::NeedsInput), 9999, 100),
        AgentState::NeedsInput
    );
    assert_eq!(
        unhooked_state(Some(AgentState::Exited), 0, 100),
        AgentState::Exited
    );
}

#[test]
fn foreground_detection_starts_a_working_agent() {
    let mut t = AgentTracker::default();
    assert!(!t.is_present());
    t.observe_foreground(Some("claude"), 1_000);
    let info = t.info().unwrap();
    assert_eq!(info.harness, "claude");
    assert_eq!(info.state, AgentState::Working);
    assert_eq!(info.since_ms, 1_000);
}

#[test]
fn losing_the_foreground_clears_unhooked_agents() {
    let mut t = AgentTracker::default();
    t.observe_foreground(Some("omp"), 1);
    t.observe_foreground(None, 2);
    assert!(t.info().is_none());
}

#[test]
fn tick_moves_unhooked_agent_to_idle_after_quiet_period() {
    let mut t = AgentTracker::default();
    t.observe_foreground(Some("claude"), 0);
    t.tick(500, 0, 1_000);
    assert_eq!(state_of(&t), Some(AgentState::Working));
    t.tick(1_500, 0, 1_000);
    assert_eq!(state_of(&t), Some(AgentState::Idle));
    assert_eq!(t.info().unwrap().since_ms, 1_500);
}

#[test]
fn bell_alert_needs_input_until_user_types() {
    let mut t = AgentTracker::default();
    t.observe_foreground(Some("claude"), 0);
    t.alert(None, 10);
    assert_eq!(state_of(&t), Some(AgentState::NeedsInput));

    t.tick(20, 0, 1_000_000);
    assert_eq!(
        state_of(&t),
        Some(AgentState::NeedsInput),
        "prompt stays pending"
    );

    t.input(30);
    assert_eq!(state_of(&t), Some(AgentState::Working));
    assert_eq!(t.info().unwrap().since_ms, 30);
}

#[test]
fn notify_alert_records_detail() {
    let mut t = AgentTracker::default();
    t.observe_foreground(Some("claude"), 0);
    t.alert(Some("approve edit?".to_string()), 5);
    let info = t.info().unwrap();
    assert_eq!(info.state, AgentState::NeedsInput);
    assert_eq!(info.detail.as_deref(), Some("approve edit?"));
}

#[test]
fn alerts_without_an_agent_are_ignored() {
    let mut t = AgentTracker::default();
    t.alert(None, 1);
    assert!(t.info().is_none());
}

#[test]
fn hook_state_is_authoritative_and_ignores_output_timing() {
    let mut t = AgentTracker::default();
    t.report(&report(Some(AgentState::NeedsInput)), 0);
    t.tick(10, 0, 1_000_000);
    assert_eq!(state_of(&t), Some(AgentState::NeedsInput));

    t.report(&report(Some(AgentState::Idle)), 20);
    t.tick(30, 29, 1_000_000);
    assert_eq!(
        state_of(&t),
        Some(AgentState::Idle),
        "hooked agents are not re-derived"
    );

    t.observe_foreground(None, 40);
    assert_eq!(
        state_of(&t),
        Some(AgentState::Idle),
        "foreground changes do not clear hooked agents"
    );
}

#[test]
fn hook_session_and_detail_are_kept() {
    let mut t = AgentTracker::default();
    t.report(
        &Report {
            harness: Some("omp".into()),
            session_id: Some("s-1".into()),
            detail: Some("thinking".into()),
            ..Report::default()
        },
        0,
    );
    let info = t.info().unwrap();
    assert_eq!(info.harness, "omp");
    assert_eq!(info.session_id.as_deref(), Some("s-1"));
    assert_eq!(info.detail.as_deref(), Some("thinking"));
    assert_eq!(info.state, AgentState::Working, "no state reported yet");
}

#[test]
fn a_reported_state_replaces_the_previous_detail() {
    let mut t = AgentTracker::default();
    let ask = Report {
        detail: Some("needs permission".into()),
        ..report(Some(AgentState::NeedsInput))
    };
    t.report(&ask, 0);
    t.report(&report(Some(AgentState::Idle)), 1);
    let info = t.info().unwrap();
    assert_eq!(info.state, AgentState::Idle);
    assert_eq!(info.detail, None);
}

#[test]
fn clear_report_drops_tracking() {
    let mut t = AgentTracker::default();
    t.report(&report(Some(AgentState::Working)), 0);
    t.report(
        &Report {
            clear: true,
            ..Report::default()
        },
        1,
    );
    assert!(t.info().is_none());
}

#[test]
fn process_exit_marks_agent_exited() {
    let mut t = AgentTracker::default();
    t.observe_foreground(Some("claude"), 0);
    t.exited(9);
    let info = t.info().unwrap();
    assert_eq!(info.state, AgentState::Exited);
    assert_eq!(info.since_ms, 9);
}

#[test]
fn report_without_harness_or_state_is_ignored() {
    let mut t = AgentTracker::default();
    t.report(&Report::default(), 0);
    assert!(t.info().is_none());
}
