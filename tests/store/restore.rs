use mpx::config::ResumeRule;
use mpx::layout::tree::{Direction, LayoutNode};
use mpx::protocol::SpawnOpts;
use mpx::store::{
    PaneSnapshot, RestorePlan, SNAPSHOT_VERSION, SessionSnapshot, WindowPlan,
    WindowSnapshot, plan_restore,
};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn shell(name: &str) -> PaneSnapshot {
    PaneSnapshot {
        name: name.into(),
        command: "bash".into(),
        args: strings(&["-l"]),
        pty: true,
        cwd: Some("/work".into()),
        env: vec![("EDITOR".into(), "vim".into())],
        remain_on_exit: Some(true),
        resume_rule: None,
        session_id: None,
    }
}

fn task(name: &str) -> PaneSnapshot {
    PaneSnapshot {
        pty: false,
        ..shell(name)
    }
}

fn tool_pane(name: &str, rule: &str, session_id: Option<&str>) -> PaneSnapshot {
    PaneSnapshot {
        command: rule.into(),
        args: vec![],
        env: vec![("USER_VAR".into(), "1".into())],
        resume_rule: Some(rule.into()),
        session_id: session_id.map(String::from),
        ..shell(name)
    }
}

fn claude() -> ResumeRule {
    ResumeRule {
        name: "claude".into(),
        detect: strings(&["claude"]),
        resume: strings(&["claude", "--resume", "{id}"]),
        fallback: strings(&["claude", "--continue"]),
        env_id: None,
        id_source: None,
    }
}

fn split(left: &str, right: &str) -> LayoutNode {
    LayoutNode::Split {
        direction: Direction::Horizontal,
        ratio: 0.5,
        left: Box::new(LayoutNode::leaf(left.into())),
        right: Box::new(LayoutNode::leaf(right.into())),
    }
}

fn window(
    name: &str,
    renamed: bool,
    layout: LayoutNode,
    panes: Vec<PaneSnapshot>,
) -> WindowSnapshot {
    WindowSnapshot {
        name: name.into(),
        renamed,
        layout,
        focused: None,
        panes,
    }
}

/// A window holding one pane as its only leaf.
fn single(name: &str, pane: PaneSnapshot) -> WindowSnapshot {
    let layout = LayoutNode::leaf(pane.name.clone());
    window(name, false, layout, vec![pane])
}

/// A window whose only pane is a plain shell; it survives pruning.
fn live(name: &str) -> WindowSnapshot {
    single(name, shell(name))
}

/// A window whose only pane is a task; pruning drops it.
fn dead(name: &str) -> WindowSnapshot {
    single(name, task(name))
}

fn focused(mut win: WindowSnapshot, pane: &str) -> WindowSnapshot {
    win.focused = Some(pane.into());
    win
}

fn restore(
    active_window: usize,
    windows: Vec<WindowSnapshot>,
    rules: &[ResumeRule],
) -> RestorePlan {
    let snapshot = SessionSnapshot {
        version: SNAPSHOT_VERSION,
        session: "main".into(),
        saved_ms: 1,
        clean_shutdown: false,
        active_window,
        last_window: None,
        windows,
    };
    plan_restore(&snapshot, rules)
}

fn restore_window(win: WindowSnapshot, rules: &[ResumeRule]) -> WindowPlan {
    let mut plan = restore(0, vec![win], rules);
    assert_eq!(plan.windows.len(), 1);
    plan.windows.remove(0)
}

fn restore_agent(pane: PaneSnapshot, rules: &[ResumeRule]) -> SpawnOpts {
    restore_window(single("main", pane), rules).panes.remove(0)
}

/// Restores a claude agent pane with the claude rule, optionally carrying a session id.
fn resumed(session_id: Option<&str>) -> SpawnOpts {
    restore_agent(tool_pane("claude", "claude", session_id), &[claude()])
}

fn assert_spawn(spawn: &SpawnOpts, command: &str, args: &[&str]) {
    assert_eq!(spawn.command, command);
    assert_eq!(spawn.args, strings(args));
}

fn only_window(plan: &RestorePlan) -> &WindowPlan {
    assert_eq!(plan.windows.len(), 1);
    &plan.windows[0]
}

#[test]
fn shell_pane_restores_original_command_cwd_and_env() {
    let win = restore_window(live("sh"), &[]);
    assert_eq!(
        win.panes,
        vec![SpawnOpts {
            name: "sh".into(),
            command: "bash".into(),
            args: vec!["-l".into()],
            pty: true,
            cwd: Some("/work".into()),
            env: vec![("EDITOR".into(), "vim".into())],
            remain_on_exit: Some(true),
        }]
    );
    assert_eq!(win.layout, LayoutNode::leaf("sh".into()));
}

#[test]
fn agent_with_session_id_resumes_through_its_rule() {
    let spawn = resumed(Some("abc"));
    assert_eq!(spawn.name, "claude");
    assert_spawn(&spawn, "claude", &["--resume", "abc"]);
    assert_eq!(spawn.cwd, Some("/work".into()));
}

#[test]
fn agent_without_session_id_uses_fallback() {
    assert_spawn(&resumed(None), "claude", &["--continue"]);
}

#[test]
fn agent_without_resume_template_uses_fallback() {
    let mut rule = claude();
    rule.resume.clear();
    let spawn = restore_agent(tool_pane("claude", "claude", Some("abc")), &[rule]);
    assert_spawn(&spawn, "claude", &["--continue"]);
}

#[test]
fn agent_with_unknown_harness_keeps_its_original_command() {
    let spawn = restore_agent(tool_pane("ghost", "unknown-tool", Some("abc")), &[claude()]);
    assert_spawn(&spawn, "unknown-tool", &[]);
    assert_eq!(spawn.env, vec![("USER_VAR".to_string(), "1".to_string())]);
}

#[test]
fn task_pane_is_pruned_from_the_layout() {
    let win = restore_window(
        window("main", false, split("live", "dead"), vec![shell("live"), task("dead")]),
        &[],
    );
    assert_eq!(win.panes.len(), 1);
    assert_eq!(win.panes[0].name, "live");
    assert_eq!(win.layout, LayoutNode::leaf("live".into()));
}

#[test]
fn window_with_only_task_panes_is_dropped() {
    let plan = restore(0, vec![dead("one"), live("two")], &[]);
    assert_eq!(plan.windows.len(), 1);
    assert_eq!(plan.windows[0].panes[0].name, "two");
    assert_eq!(plan.active_window, 0);
}

#[test]
fn focus_falls_back_to_first_remaining_pane_when_focused_was_pruned() {
    let win = window("main", false, split("kept", "gone"), vec![shell("kept"), task("gone")]);
    let win = restore_window(focused(win, "gone"), &[]);
    assert_eq!(win.focused, Some("kept".into()));
}

#[test]
fn focus_is_kept_when_the_focused_pane_survives() {
    let win = restore_window(focused(live("p"), "p"), &[]);
    assert_eq!(win.focused, Some("p".into()));
}

#[test]
fn active_window_is_remapped_past_dropped_windows() {
    let plan = restore(2, vec![dead("zero"), live("one"), live("two")], &[]);
    assert_eq!(plan.windows.len(), 2);
    assert_eq!(plan.active_window, 1);
}

#[test]
fn active_window_that_was_dropped_moves_to_the_next_survivor() {
    let plan = restore(0, vec![dead("zero"), live("one")], &[]);
    assert_eq!(plan.active_window, 0);
    assert_eq!(only_window(&plan).panes[0].name, "one");
}

#[test]
fn active_window_past_the_end_is_clamped_to_the_last_survivor() {
    let plan = restore(99, vec![live("one")], &[]);
    assert_eq!(plan.active_window, 0);
}

#[test]
fn renamed_window_keeps_its_name_and_auto_named_window_does_not() {
    let manual = WindowSnapshot {
        renamed: true,
        ..live("manual")
    };
    let plan = restore(0, vec![manual, live("auto")], &[]);
    assert_eq!(plan.windows[0].name.as_deref(), Some("manual"));
    assert_eq!(plan.windows[1].name, None);
}

#[test]
fn empty_session_yields_empty_plan() {
    let plan = restore(0, vec![], &[]);
    assert_eq!(plan.windows, Vec::<WindowPlan>::new());
}
