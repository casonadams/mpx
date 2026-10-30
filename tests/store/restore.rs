use mpx::config::ResumeRule;
use mpx::layout::tree::{Direction, LayoutNode};
use mpx::protocol::SpawnOpts;
use mpx::store::{
    PaneSnapshot, RestorePlan, SNAPSHOT_VERSION, SessionSnapshot, WindowPlan,
    WindowSnapshot, plan_restore,
};

fn shell(name: &str) -> PaneSnapshot {
    PaneSnapshot {
        name: name.into(),
        command: "bash".into(),
        args: vec!["-l".into()],
        pty: true,
        cwd: Some("/work".into()),
        env: vec![("EDITOR".into(), "vim".into())],
        remain_on_exit: Some(true),
        tmux_compat: false,
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
        tmux_compat: true,
        resume_rule: Some(rule.into()),
        session_id: session_id.map(String::from),
        ..shell(name)
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

fn session(active_window: usize, windows: Vec<WindowSnapshot>) -> SessionSnapshot {
    SessionSnapshot {
        version: SNAPSHOT_VERSION,
        session: "main".into(),
        saved_ms: 1,
        clean_shutdown: false,
        active_window,
        last_window: None,
        windows,
    }
}

fn claude() -> ResumeRule {
    ResumeRule {
        name: "claude".into(),
        detect: vec!["claude".into()],
        resume: vec!["claude".into(), "--resume".into(), "{id}".into()],
        fallback: vec!["claude".into(), "--continue".into()],
        env_id: None,
        id_source: None,
    }
}

fn restore(snapshot: &SessionSnapshot, rules: &[ResumeRule]) -> RestorePlan {
    plan_restore(snapshot, rules)
}

fn only_window(plan: &RestorePlan) -> &WindowPlan {
    assert_eq!(plan.windows.len(), 1);
    &plan.windows[0]
}

#[test]
fn shell_pane_restores_original_command_cwd_and_env() {
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("sh".into()),
            vec![shell("sh")],
        )],
    );
    let plan = restore(&snap, &[]);
    let win = only_window(&plan);
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
            tmux_compat: false,
        }]
    );
    assert_eq!(win.layout, LayoutNode::leaf("sh".into()));
}

#[test]
fn agent_with_session_id_resumes_through_its_rule() {
    let pane = tool_pane("claude", "claude", Some("abc"));
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("claude".into()),
            vec![pane],
        )],
    );
    let plan = restore(&snap, &[claude()]);
    let spawn = &only_window(&plan).panes[0];

    assert_eq!(spawn.name, "claude");
    assert_eq!(spawn.command, "claude");
    assert_eq!(spawn.args, vec!["--resume".to_string(), "abc".to_string()]);
    assert_eq!(spawn.cwd, Some("/work".into()));
    assert!(spawn.tmux_compat);
}

#[test]
fn agent_without_session_id_uses_fallback() {
    let pane = tool_pane("claude", "claude", None);
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("claude".into()),
            vec![pane],
        )],
    );
    let plan = restore(&snap, &[claude()]);
    let spawn = &only_window(&plan).panes[0];
    assert_eq!(spawn.command, "claude");
    assert_eq!(spawn.args, vec!["--continue".to_string()]);
}

#[test]
fn agent_without_resume_template_uses_fallback() {
    let mut rule = claude();
    rule.resume.clear();
    let pane = tool_pane("claude", "claude", Some("abc"));
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("claude".into()),
            vec![pane],
        )],
    );
    let plan = restore(&snap, &[rule]);
    assert_eq!(only_window(&plan).panes[0].args, vec!["--continue".to_string()]);
}

#[test]
fn agent_with_unknown_harness_keeps_its_original_command() {
    let pane = tool_pane("ghost", "unknown-tool", Some("abc"));
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("ghost".into()),
            vec![pane],
        )],
    );
    let plan = restore(&snap, &[claude()]);
    let spawn = &only_window(&plan).panes[0];
    assert_eq!(spawn.command, "unknown-tool");
    assert!(spawn.args.is_empty());
    assert_eq!(spawn.env, vec![("USER_VAR".to_string(), "1".to_string())]);
}

#[test]
fn task_pane_is_pruned_from_the_layout() {
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            split("live", "dead"),
            vec![shell("live"), task("dead")],
        )],
    );
    let plan = restore(&snap, &[]);
    let win = only_window(&plan);
    assert_eq!(win.panes.len(), 1);
    assert_eq!(win.panes[0].name, "live");
    assert_eq!(win.layout, LayoutNode::leaf("live".into()));
}

#[test]
fn window_with_only_task_panes_is_dropped() {
    let snap = session(
        0,
        vec![
            window(
                "one",
                false,
                LayoutNode::leaf("dead".into()),
                vec![task("dead")],
            ),
            window(
                "two",
                false,
                LayoutNode::leaf("live".into()),
                vec![shell("live")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows.len(), 1);
    assert_eq!(plan.windows[0].panes[0].name, "live");
    assert_eq!(plan.active_window, 0);
}

#[test]
fn focus_falls_back_to_first_remaining_pane_when_focused_was_pruned() {
    let mut win = window(
        "main",
        false,
        split("kept", "gone"),
        vec![shell("kept"), task("gone")],
    );
    win.focused = Some("gone".into());
    let snap = session(0, vec![win]);
    let plan = restore(&snap, &[]);
    assert_eq!(only_window(&plan).focused, Some("kept".into()));
}

#[test]
fn focus_is_kept_when_the_focused_pane_survives() {
    let mut win = window("main", false, LayoutNode::leaf("p".into()), vec![shell("p")]);
    win.focused = Some("p".into());
    let snap = session(0, vec![win]);
    let plan = restore(&snap, &[]);
    assert_eq!(only_window(&plan).focused, Some("p".into()));
}

#[test]
fn active_window_is_remapped_past_dropped_windows() {
    let snap = session(
        2,
        vec![
            window(
                "zero",
                false,
                LayoutNode::leaf("task0".into()),
                vec![task("task0")],
            ),
            window(
                "one",
                false,
                LayoutNode::leaf("live1".into()),
                vec![shell("live1")],
            ),
            window(
                "two",
                false,
                LayoutNode::leaf("live2".into()),
                vec![shell("live2")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows.len(), 2);
    assert_eq!(plan.active_window, 1);
}

#[test]
fn active_window_that_was_dropped_moves_to_the_next_survivor() {
    let snap = session(
        0,
        vec![
            window(
                "zero",
                false,
                LayoutNode::leaf("dead0".into()),
                vec![task("dead0")],
            ),
            window(
                "one",
                false,
                LayoutNode::leaf("live1".into()),
                vec![shell("live1")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.active_window, 0);
    assert_eq!(only_window(&plan).panes[0].name, "live1");
}

#[test]
fn active_window_past_the_end_is_clamped_to_the_last_survivor() {
    let snap = session(
        99,
        vec![window(
            "one",
            false,
            LayoutNode::leaf("live1".into()),
            vec![shell("live1")],
        )],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.active_window, 0);
}

#[test]
fn renamed_window_keeps_its_name_and_auto_named_window_does_not() {
    let snap = session(
        0,
        vec![
            window(
                "manual",
                true,
                LayoutNode::leaf("p".into()),
                vec![shell("p")],
            ),
            window(
                "auto",
                false,
                LayoutNode::leaf("q".into()),
                vec![shell("q")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows[0].name.as_deref(), Some("manual"));
    assert_eq!(plan.windows[1].name, None);
}

#[test]
fn empty_session_yields_empty_plan() {
    let snap = session(0, vec![]);
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows, Vec::<WindowPlan>::new());
}
