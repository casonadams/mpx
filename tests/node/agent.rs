use mpx::node::agent::{AgentTracker, Report, record_edit, unhooked_state};
use mpx::protocol::{AgentState, EDIT_WINDOW_MS, FileEdit, MAX_EDITS};

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

fn edit(path: &str, at_ms: u64) -> FileEdit {
    FileEdit {
        path: path.to_string(),
        at_ms,
    }
}

#[test]
fn record_edit_moves_a_repeated_path_to_the_newest_end() {
    let mut edits = vec![edit("a.rs", 1), edit("b.rs", 2)];
    record_edit(&mut edits, "a.rs", 3);
    assert_eq!(edits, vec![edit("b.rs", 2), edit("a.rs", 3)]);
}

#[test]
fn record_edit_drops_edits_outside_the_window_and_beyond_the_cap() {
    let mut edits = vec![edit("old.rs", 0)];
    record_edit(&mut edits, "new.rs", EDIT_WINDOW_MS);
    assert_eq!(edits, vec![edit("new.rs", EDIT_WINDOW_MS)]);

    let mut many: Vec<FileEdit> = (0..MAX_EDITS as u64)
        .map(|i| edit(&format!("f{i}.rs"), i))
        .collect();
    record_edit(&mut many, "last.rs", 100);
    assert_eq!(many.len(), MAX_EDITS);
    assert_eq!(many[0].path, "f1.rs");
    assert_eq!(many[MAX_EDITS - 1].path, "last.rs");
}

#[test]
fn hook_focus_keeps_the_latest_task_and_reply_and_collects_edits() {
    let mut t = AgentTracker::default();
    t.report(
        &Report {
            task: Some("migrate billing".into()),
            ..report(Some(AgentState::Working))
        },
        10,
    );
    t.report(
        &Report {
            edited: Some("/repo/src/a.rs".into()),
            ..report(None)
        },
        20,
    );
    t.report(
        &Report {
            reply: Some("done, tests pass".into()),
            ..report(Some(AgentState::Idle))
        },
        30,
    );
    let focus = t.info().expect("agent").focus;
    assert_eq!(focus.task.as_deref(), Some("migrate billing"));
    assert_eq!(focus.reply.as_deref(), Some("done, tests pass"));
    assert_eq!(focus.edits, vec![edit("/repo/src/a.rs", 20)]);
}

#[test]
fn focus_without_an_agent_is_ignored_and_clearing_forgets_it() {
    let mut t = AgentTracker::default();
    t.report(
        &Report {
            task: Some("x".into()),
            ..Report::default()
        },
        0,
    );
    assert!(t.info().is_none());
    t.report(
        &Report {
            task: Some("x".into()),
            ..report(Some(AgentState::Working))
        },
        0,
    );
    t.report(
        &Report {
            clear: true,
            ..Report::default()
        },
        1,
    );
    t.report(&report(Some(AgentState::Working)), 2);
    assert_eq!(t.info().expect("agent").focus.task, None);
}
