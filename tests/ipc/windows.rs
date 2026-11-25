use std::path::Path;

use mpx::ipc::{endpoint_in, sessions_from_names, socket_dir};

fn entry_of(endpoint: &str) -> &str {
    endpoint.strip_prefix(r"\\.\pipe\").unwrap()
}

#[test]
fn a_default_session_named_like_a_custom_dir_pipe_is_listed() {
    let sessions = sessions_from_names(&socket_dir(), ["mpx-0123abcd-work"]);
    let got: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(got, ["0123abcd-work"]);
}

#[test]
fn custom_dir_sessions_are_listed_only_in_their_dir() {
    let dir = Path::new(r"C:\custom");
    let own = entry_of(&endpoint_in(dir, "work")).to_string();
    let sessions = sessions_from_names(dir, [own.as_str()]);
    let got: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(got, ["work"]);
    assert!(sessions_from_names(&socket_dir(), [own.as_str()]).is_empty());
}
