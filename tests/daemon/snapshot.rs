use mpx::daemon::snapshot::{PaneRecord, build_snapshot};
use mpx::layout::{Direction, Workspace};
use mpx::protocol::SpawnOpts;

use super::support::task;

fn record(name: &str, running: bool, cwd: Option<&str>) -> PaneRecord {
    PaneRecord {
        name: name.to_string(),
        opts: SpawnOpts {
            pty: true,
            cwd: Some("/start".to_string()),
            env: vec![("API_KEY".to_string(), "v".to_string())],
            remain_on_exit: Some(true),
            ..task(name, "claude", &["--resume"])
        },
        running,
        cwd: cwd.map(str::to_string),
        resume_rule: None,
        session_id: None,
    }
}

#[test]
fn snapshot_keeps_running_panes_with_their_live_cwd_and_user_settings() {
    let mut ws = Workspace::new("main".to_string());
    ws.new_window("work".to_string(), "a".to_string());
    ws.split_pane(
        Some("a"),
        "b".to_string(),
        Direction::Horizontal,
        None,
        true,
    )
    .unwrap();
    let mut a = record("a", true, None);
    a.resume_rule = Some("claude".to_string());
    a.session_id = Some("s-1".to_string());
    let panes = vec![a, record("b", true, Some("/live"))];

    let snap = build_snapshot("main", 42, false, &ws, &panes);

    assert_eq!(snap.session, "main");
    assert_eq!(snap.saved_ms, 42);
    assert!(!snap.clean_shutdown);
    assert_eq!(snap.windows.len(), 1);
    let win = &snap.windows[0];
    assert_eq!(win.name, "work");
    assert_eq!(win.layout.all_leaves(), ["a", "b"]);
    let a = &win.panes[0];
    assert_eq!(
        a.cwd.as_deref(),
        Some("/start"),
        "falls back to the spawn cwd"
    );
    assert_eq!(a.env, [("API_KEY".to_string(), "v".to_string())]);
    assert_eq!(a.command, "claude");
    assert_eq!(a.args, ["--resume"]);
    assert!(a.pty);
    assert_eq!(a.remain_on_exit, Some(true));
    assert_eq!(a.session_id.as_deref(), Some("s-1"));
    assert_eq!(win.panes[1].cwd.as_deref(), Some("/live"));
}

#[test]
fn exited_panes_leave_the_layout_and_empty_windows_are_dropped() {
    let mut ws = Workspace::new("main".to_string());
    ws.new_window("first".to_string(), "a".to_string());
    ws.split_pane(
        Some("a"),
        "b".to_string(),
        Direction::Horizontal,
        None,
        true,
    )
    .unwrap();
    ws.new_window("gone".to_string(), "c".to_string());
    ws.select_window(1);
    let panes = vec![
        record("a", true, None),
        record("b", false, None),
        record("c", false, None),
    ];

    let snap = build_snapshot("main", 0, true, &ws, &panes);

    assert!(snap.clean_shutdown);
    assert_eq!(
        snap.windows.len(),
        1,
        "window with only exited panes is dropped"
    );
    assert_eq!(snap.windows[0].layout.all_leaves(), ["a"]);
    assert_eq!(snap.windows[0].panes.len(), 1);
    assert_eq!(
        snap.active_window, 0,
        "active window falls back when dropped"
    );
}

#[test]
fn focus_on_an_exited_pane_is_not_recorded() {
    let mut ws = Workspace::new("main".to_string());
    ws.new_window("w".to_string(), "a".to_string());
    ws.split_pane(
        Some("a"),
        "b".to_string(),
        Direction::Horizontal,
        None,
        true,
    )
    .unwrap();
    assert_eq!(ws.windows[0].focused.as_deref(), Some("b"));
    let panes = vec![record("a", true, None), record("b", false, None)];

    let snap = build_snapshot("main", 0, false, &ws, &panes);

    assert_eq!(snap.windows[0].focused, None);
}
