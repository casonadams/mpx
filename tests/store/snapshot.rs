use mpx::layout::tree::{Direction, LayoutNode};
use mpx::store::{
    AgentSnapshot, PaneSnapshot, SNAPSHOT_VERSION, SessionSnapshot, SnapshotStore, WindowSnapshot,
    decode_snapshot, encode_snapshot,
};

fn sample() -> SessionSnapshot {
    SessionSnapshot {
        version: SNAPSHOT_VERSION,
        session: "work/../dev".into(),
        saved_ms: 42,
        clean_shutdown: false,
        active_window: 1,
        windows: vec![WindowSnapshot {
            name: "editor".into(),
            renamed: true,
            layout: LayoutNode::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                left: Box::new(LayoutNode::leaf("shell".into())),
                right: Box::new(LayoutNode::leaf("claude".into())),
            },
            focused: Some("claude".into()),
            panes: vec![
                PaneSnapshot {
                    name: "shell".into(),
                    command: "bash".into(),
                    args: vec!["-l".into()],
                    pty: true,
                    cwd: Some("/tmp".into()),
                    env: vec![("EDITOR".into(), "vim".into())],
                    remain_on_exit: None,
                    tmux_compat: false,
                    agent: None,
                },
                PaneSnapshot {
                    name: "claude".into(),
                    command: "claude".into(),
                    args: vec![],
                    pty: true,
                    cwd: None,
                    env: vec![],
                    remain_on_exit: Some(true),
                    tmux_compat: true,
                    agent: Some(AgentSnapshot {
                        harness: "claude".into(),
                        session_id: Some("abc".into()),
                    }),
                },
            ],
        }],
    }
}

fn session_dir(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("session")
}

fn snapshot_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    session_dir(dir).join("snapshot.json")
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::in_dir(&session_dir(&dir));
    store.save(&sample()).unwrap();
    assert_eq!(store.load().unwrap(), Some(sample()));
}

#[test]
fn missing_snapshot_loads_as_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(snapshot_file(&dir));
    assert_eq!(store.load().unwrap(), None);
}

#[test]
fn resave_replaces_contents_without_leaving_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(snapshot_file(&dir));
    store.save(&sample()).unwrap();
    let mut newer = sample();
    newer.saved_ms = 99;
    newer.clean_shutdown = true;
    store.save(&newer).unwrap();

    assert_eq!(store.load().unwrap(), Some(newer));
    let names: Vec<String> = std::fs::read_dir(dir.path().join("session"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["snapshot.json".to_string()]);
}

#[test]
fn corrupt_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = snapshot_file(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{\"version\": 1, \"session\": ").unwrap();
    assert!(SnapshotStore::new(path).load().is_err());
}

#[test]
fn newer_version_is_rejected_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = snapshot_file(&dir);
    let mut future = sample();
    future.version = SNAPSHOT_VERSION + 1;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, encode_snapshot(&future).unwrap()).unwrap();
    let err = SnapshotStore::new(path).load().unwrap_err();
    assert!(err.contains("newer"), "{err}");
}

#[test]
fn encode_decode_round_trips() {
    let text = encode_snapshot(&sample()).unwrap();
    assert_eq!(decode_snapshot(&text).unwrap(), sample());
}

#[test]
fn decode_rejects_newer_version_before_reading_body() {
    let text = r#"{"version": 2, "layout_v2": true}"#;
    assert!(decode_snapshot(text).is_err());
}

#[test]
fn decode_rejects_missing_version() {
    assert!(decode_snapshot(r#"{"session": "s"}"#).is_err());
}
