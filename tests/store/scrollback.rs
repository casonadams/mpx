use mpx::store::{PaneScrollback, ScrollbackStore};

fn saved(log: &[u8]) -> PaneScrollback {
    PaneScrollback {
        size: (120, 40),
        log: log.to_vec(),
    }
}

#[test]
fn saved_scrollback_loads_back_byte_for_byte_with_its_size() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    assert_eq!(store.load("shell").unwrap(), None);

    let log = saved(b"\x1b[31mred\x1b[0m\r\nline\n\xff\xfe");
    store.save("shell", &log).unwrap();
    store.save("a/../b", &saved(b"other")).unwrap();

    assert_eq!(store.load("shell").unwrap(), Some(log));
    assert_eq!(store.load("a/../b").unwrap(), Some(saved(b"other")));
}

#[test]
fn retain_deletes_the_scrollback_of_every_other_pane() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    store.retain(&["kept".to_string()]).unwrap();
    for pane in ["kept", "gone", "also/gone"] {
        store.save(pane, &saved(pane.as_bytes())).unwrap();
    }

    store.retain(&["kept".to_string()]).unwrap();

    assert_eq!(store.load("kept").unwrap(), Some(saved(b"kept")));
    assert_eq!(store.load("gone").unwrap(), None);
    assert_eq!(store.load("also/gone").unwrap(), None);
    store.retain(&[]).unwrap();
    assert_eq!(store.load("kept").unwrap(), None);
}

#[test]
fn a_file_without_the_scrollback_header_is_an_error() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    store.save("shell", &saved(b"")).unwrap();
    let file = state.path().join("scrollback").join("shell");
    for corrupt in [
        "not a header\nbody",
        "mpx-scrollback 1 80\nbody",
        "no newline",
    ] {
        std::fs::write(&file, corrupt).unwrap();
        assert!(store.load("shell").is_err(), "{corrupt:?}");
    }
}

#[test]
fn header_sizes_outside_the_window_bounds_are_rejected() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    store.save("shell", &saved(b"")).unwrap();
    let file = state.path().join("scrollback").join("shell");
    for header in ["0 40", "120 0", "65535 65535", "10001 40"] {
        std::fs::write(&file, format!("mpx-scrollback 1 {header}\nbody")).unwrap();
        assert!(store.load("shell").is_err(), "{header}");
    }
    std::fs::write(&file, "mpx-scrollback 1 10000 1\nbody").unwrap();
    assert!(store.load("shell").unwrap().is_some());
}

#[test]
fn pane_names_differing_only_by_case_keep_their_own_scrollback() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    store.save("Shell", &saved(b"upper")).unwrap();
    store.save("shell", &saved(b"lower")).unwrap();
    assert_eq!(store.load("Shell").unwrap(), Some(saved(b"upper")));
    assert_eq!(store.load("shell").unwrap(), Some(saved(b"lower")));
    for entry in std::fs::read_dir(state.path().join("scrollback")).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(!name.bytes().any(|b| b.is_ascii_uppercase()), "{name}");
    }
}

#[test]
fn saving_replaces_a_stale_temp_file_left_by_a_crash() {
    let state = tempfile::tempdir().unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    let dir = state.path().join("scrollback");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".shell.tmp"), "stale").unwrap();
    store.save("shell", &saved(b"fresh")).unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["shell".to_string()]);
}

#[cfg(unix)]
#[test]
fn saved_scrollback_is_owner_only_even_in_a_permissive_directory() {
    use std::os::unix::fs::PermissionsExt;
    let state = tempfile::tempdir().unwrap();
    let dir = state.path().join("scrollback");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = ScrollbackStore::in_dir(state.path());
    store.save("shell", &saved(b"private")).unwrap();

    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir.join("shell")), 0o600);
    assert_eq!(mode(&dir), 0o700);
}

#[test]
fn an_unwritable_state_dir_fails_to_save_and_to_purge() {
    let state = tempfile::tempdir().unwrap();
    let blocker = state.path().join("not-a-dir");
    std::fs::write(&blocker, "file").unwrap();
    let store = ScrollbackStore::in_dir(&blocker);
    assert!(store.save("shell", &saved(b"x")).is_err());
    assert!(store.retain(&[]).is_err());
}
