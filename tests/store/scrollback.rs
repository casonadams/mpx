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
