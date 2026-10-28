#![cfg(unix)]

use std::path::Path;

use mpx::ipc::{endpoint_in, sessions_from_names};

const DIR: &str = "/run/mpx-test";

#[test]
fn socket_entries_become_sorted_unique_sessions() {
    let names = ["work.sock", "main.sock", "notes.txt", ".sock", "main.sock"];
    let sessions = sessions_from_names(Path::new(DIR), names);
    let got: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(got, ["main", "work"]);
}

#[test]
fn session_endpoints_live_in_the_scanned_directory() {
    let sessions = sessions_from_names(Path::new(DIR), ["alpha.sock"]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].endpoint, "/run/mpx-test/alpha.sock");
    assert_eq!(sessions[0].endpoint, endpoint_in(Path::new(DIR), "alpha"));
}

#[test]
fn no_entries_means_no_sessions() {
    assert!(sessions_from_names(Path::new(DIR), Vec::<String>::new()).is_empty());
}
