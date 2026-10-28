use mpx::agents::{Harness, LaunchOptions, LaunchSpec};
use mpx::layout::tree::{Direction, LayoutNode};
use mpx::protocol::SpawnOpts;
use mpx::store::{
    AgentSnapshot, PaneSnapshot, RestorePlan, SNAPSHOT_VERSION, SessionSnapshot, WindowPlan,
    WindowSnapshot, plan_restore_with,
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
        agent: None,
    }
}

fn task(name: &str) -> PaneSnapshot {
    PaneSnapshot {
        pty: false,
        ..shell(name)
    }
}

fn agent(name: &str, harness: &str, session_id: Option<&str>) -> PaneSnapshot {
    PaneSnapshot {
        command: harness.into(),
        args: vec![],
        env: vec![("USER_VAR".into(), "1".into())],
        tmux_compat: true,
        agent: Some(AgentSnapshot {
            harness: harness.into(),
            session_id: session_id.map(String::from),
        }),
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
        windows,
    }
}

fn claude() -> Harness {
    Harness {
        name: "claude".into(),
        detect: vec![],
        command: vec!["claude".into()],
        resume: vec!["claude".into(), "--resume".into(), "{id}".into()],
        session_id_args: vec![],
        store: None,
    }
}

/// Stands in for `agents::launch`: resumes with `--resume <id>` on top of the harness command.
fn fake_launch(h: &Harness, resume: Option<&str>, opts: LaunchOptions) -> LaunchSpec {
    let mut argv = h.command.clone();
    if let Some(id) = resume {
        argv.push("--resume".into());
        argv.push(id.into());
    }
    LaunchSpec {
        name: h.name.clone(),
        argv,
        env: vec![("MPX_AGENT_HARNESS".into(), h.name.clone())],
        tmux_compat: opts.teams,
        session_id: resume.map(String::from),
    }
}

fn restore(snapshot: &SessionSnapshot, harnesses: &[Harness]) -> RestorePlan {
    plan_restore_with(snapshot, harnesses, fake_launch)
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
fn agent_with_session_id_resumes_through_its_harness() {
    let pane = agent("claude", "claude", Some("abc"));
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
    assert_eq!(
        spawn.env,
        vec![
            ("MPX_AGENT_HARNESS".to_string(), "claude".to_string()),
            ("USER_VAR".to_string(), "1".to_string()),
        ]
    );
}

#[test]
fn agent_without_session_id_starts_fresh() {
    let pane = agent("claude", "claude", None);
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
    assert!(spawn.args.is_empty());
}

#[test]
fn agent_without_resume_template_starts_fresh_even_with_an_id() {
    let mut harness = claude();
    harness.resume.clear();
    let pane = agent("claude", "claude", Some("abc"));
    let snap = session(
        0,
        vec![window(
            "main",
            false,
            LayoutNode::leaf("claude".into()),
            vec![pane],
        )],
    );
    let plan = restore(&snap, &[harness]);
    assert!(only_window(&plan).panes[0].args.is_empty());
}

#[test]
fn agent_with_unknown_harness_keeps_its_original_command() {
    let pane = agent("ghost", "unknown-harness", Some("abc"));
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
    assert_eq!(spawn.command, "unknown-harness");
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
            split("shell", "task"),
            vec![shell("shell"), task("task")],
        )],
    );
    let plan = restore(&snap, &[]);
    let win = only_window(&plan);
    assert_eq!(win.layout, LayoutNode::leaf("shell".into()));
    assert_eq!(win.panes.len(), 1);
    assert_eq!(win.panes[0].name, "shell");
}

#[test]
fn window_with_only_task_panes_is_dropped() {
    let snap = session(
        0,
        vec![
            window(
                "tasks",
                false,
                LayoutNode::leaf("task".into()),
                vec![task("task")],
            ),
            window(
                "main",
                false,
                LayoutNode::leaf("sh".into()),
                vec![shell("sh")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows.len(), 1);
    assert_eq!(plan.windows[0].panes[0].name, "sh");
}

#[test]
fn focus_falls_back_to_first_remaining_pane_when_focused_was_pruned() {
    let mut win = window(
        "main",
        false,
        split("task", "shell"),
        vec![task("task"), shell("shell")],
    );
    win.focused = Some("task".into());
    let plan = restore(&session(0, vec![win]), &[]);
    assert_eq!(only_window(&plan).focused, Some("shell".into()));
}

#[test]
fn focus_is_kept_when_the_focused_pane_survives() {
    let mut win = window("main", false, split("a", "b"), vec![shell("a"), shell("b")]);
    win.focused = Some("b".into());
    let plan = restore(&session(0, vec![win]), &[]);
    assert_eq!(only_window(&plan).focused, Some("b".into()));
}

#[test]
fn active_window_is_remapped_past_dropped_windows() {
    let snap = session(
        2,
        vec![
            window("one", false, LayoutNode::leaf("a".into()), vec![shell("a")]),
            window("dead", false, LayoutNode::leaf("t".into()), vec![task("t")]),
            window(
                "three",
                false,
                LayoutNode::leaf("c".into()),
                vec![shell("c")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.active_window, 1);
    assert_eq!(plan.windows[1].panes[0].name, "c");
}

#[test]
fn active_window_that_was_dropped_moves_to_the_next_survivor() {
    let snap = session(
        1,
        vec![
            window("one", false, LayoutNode::leaf("a".into()), vec![shell("a")]),
            window("dead", false, LayoutNode::leaf("t".into()), vec![task("t")]),
            window(
                "three",
                false,
                LayoutNode::leaf("c".into()),
                vec![shell("c")],
            ),
        ],
    );
    assert_eq!(restore(&snap, &[]).active_window, 1);
}

#[test]
fn active_window_past_the_end_is_clamped_to_the_last_survivor() {
    let snap = session(
        9,
        vec![
            window("one", false, LayoutNode::leaf("a".into()), vec![shell("a")]),
            window("dead", false, LayoutNode::leaf("t".into()), vec![task("t")]),
        ],
    );
    assert_eq!(restore(&snap, &[]).active_window, 0);
}

#[test]
fn renamed_window_keeps_its_name_and_auto_named_window_does_not() {
    let snap = session(
        0,
        vec![
            window("work", true, LayoutNode::leaf("a".into()), vec![shell("a")]),
            window(
                "bash",
                false,
                LayoutNode::leaf("b".into()),
                vec![shell("b")],
            ),
        ],
    );
    let plan = restore(&snap, &[]);
    assert_eq!(plan.windows[0].name, Some("work".into()));
    assert_eq!(plan.windows[1].name, None);
}

#[test]
fn empty_session_yields_empty_plan() {
    let plan = restore(&session(3, vec![]), &[]);
    assert!(plan.windows.is_empty());
    assert_eq!(plan.active_window, 0);
}
