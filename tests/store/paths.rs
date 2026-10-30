use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use mpx::store::{adopt_legacy_dir, component, state_dir_with};

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), OsString::from(v)))
        .collect();
    move |key: &str| map.get(key).cloned()
}

#[test]
fn mpx_state_dir_wins_over_xdg_and_home() {
    let dir = state_dir_with(
        env_of(&[("MPX_STATE_DIR", "/custom"), ("XDG_STATE_HOME", "/xdg")]),
        Some(PathBuf::from("/home/u")),
    );
    assert_eq!(dir, PathBuf::from("/custom"));
}

#[test]
fn xdg_state_home_is_used_before_home() {
    let dir = state_dir_with(
        env_of(&[("XDG_STATE_HOME", "/xdg")]),
        Some(PathBuf::from("/home/u")),
    );
    assert_eq!(dir, PathBuf::from("/xdg").join("mpx"));
}

#[test]
fn falls_back_to_home_local_state() {
    let dir = state_dir_with(env_of(&[]), Some(PathBuf::from("/home/u")));
    assert_eq!(
        dir,
        PathBuf::from("/home/u")
            .join(".local")
            .join("state")
            .join("mpx")
    );
}

#[test]
fn empty_variables_are_ignored() {
    let dir = state_dir_with(
        env_of(&[("MPX_STATE_DIR", ""), ("XDG_STATE_HOME", "")]),
        Some(PathBuf::from("/home/u")),
    );
    assert!(dir.starts_with("/home/u"));
}

#[test]
fn plain_names_are_unchanged() {
    assert_eq!(component("main-1_x"), "main-1_x");
}

#[test]
fn separators_and_dot_segments_are_encoded() {
    for name in ["../etc", "a/b", "..", ".", "a\\b", "C:x"] {
        let c = component(name);
        assert!(
            !c.contains('/') && !c.contains('\\') && !c.contains(':'),
            "{c}"
        );
        assert_ne!(c, ".");
        assert_ne!(c, "..");
    }
}

#[test]
fn distinct_names_map_to_distinct_components() {
    assert_ne!(component("a/b"), component("a_b"));
    assert_ne!(component("a%2Fb"), component("a/b"));
}

#[test]
fn empty_name_gets_a_component() {
    assert!(!component("").is_empty());
}

#[test]
fn very_long_names_are_bounded() {
    let long = "x/".repeat(300);
    assert!(component(&long).len() <= 128);
    assert_ne!(component(&long), component(&"x/".repeat(301)));
}

#[test]
fn legacy_state_moves_only_when_the_new_name_has_none() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("main");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("snapshot.json"), "old").unwrap();

    assert!(adopt_legacy_dir(root.path(), "main", "0"));
    assert!(!legacy.exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("0/snapshot.json")).unwrap(),
        "old"
    );

    std::fs::create_dir(&legacy).unwrap();
    assert!(!adopt_legacy_dir(root.path(), "main", "0"), "0 already has state");
    assert!(legacy.exists());
    assert!(!adopt_legacy_dir(root.path(), "gone", "1"));
}
