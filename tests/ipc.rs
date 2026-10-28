#![cfg(unix)]

use mpx::ipc::{default_endpoint, sessions_from_names};

#[test]
fn socket_entries_become_sorted_unique_sessions() {
    let names = ["work.sock", "main.sock", "notes.txt", ".sock", "main.sock"];
    let sessions = sessions_from_names(names);
    let got: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(got, ["main", "work"]);
}

#[test]
fn session_endpoints_are_the_default_endpoints() {
    let sessions = sessions_from_names(["alpha.sock"]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].endpoint, default_endpoint("alpha"));
}

#[test]
fn no_entries_means_no_sessions() {
    assert!(sessions_from_names(Vec::<String>::new()).is_empty());
}
