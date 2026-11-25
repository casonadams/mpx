use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use mpx::ipc::{IpcListener, check_endpoint, endpoint_in, sessions_from_names};

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

fn private_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_socket_dir_is_created_owner_only() {
    let root = private_root();
    let dir = root.path().join("mpx");
    let endpoint = dir.join("0.sock");
    assert!(IpcListener::bind(endpoint.to_str().unwrap()).is_ok());
    assert_eq!(mode_of(&dir), 0o700);
}

#[tokio::test]
async fn a_symlinked_socket_dir_is_refused_and_its_target_left_alone() {
    let root = private_root();
    let target = root.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = root.path().join("mpx");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let endpoint = link.join("0.sock");
    assert!(IpcListener::bind(endpoint.to_str().unwrap()).is_err());
    assert_eq!(mode_of(&target), 0o755);
}

#[tokio::test]
async fn a_regular_file_in_the_socket_dir_place_is_refused() {
    let root = private_root();
    let dir = root.path().join("mpx");
    std::fs::write(&dir, "not a dir").unwrap();
    let endpoint = dir.join("0.sock");
    assert!(IpcListener::bind(endpoint.to_str().unwrap()).is_err());
}

#[tokio::test]
async fn a_live_daemon_is_not_replaced_by_a_second_bind() {
    let root = private_root();
    let endpoint = root.path().join("0.sock");
    let endpoint = endpoint.to_str().unwrap();
    let _first = IpcListener::bind(endpoint).unwrap();

    let second = IpcListener::bind(endpoint);
    assert_eq!(second.err().map(|e| e.kind()), Some(ErrorKind::AddrInUse));
    assert!(std::os::unix::net::UnixStream::connect(endpoint).is_ok());
}

#[tokio::test]
async fn a_socket_left_by_a_dead_daemon_is_replaced() {
    let root = private_root();
    let endpoint = root.path().join("0.sock");
    drop(std::os::unix::net::UnixListener::bind(&endpoint).unwrap());
    assert!(std::os::unix::net::UnixStream::connect(&endpoint).is_err());

    assert!(IpcListener::bind(endpoint.to_str().unwrap()).is_ok());
}

#[tokio::test]
async fn dropping_a_superseded_listener_keeps_the_path_of_its_successor() {
    let root = private_root();
    let endpoint = root.path().join("0.sock");
    let first = IpcListener::bind(endpoint.to_str().unwrap()).unwrap();
    std::fs::remove_file(&endpoint).unwrap();
    let _second = IpcListener::bind(endpoint.to_str().unwrap()).unwrap();

    drop(first);
    assert!(std::os::unix::net::UnixStream::connect(&endpoint).is_ok());
}

#[test]
fn an_endpoint_past_the_unix_socket_path_limit_is_refused() {
    let dir = Path::new("/tmp/mpx-test");
    assert!(check_endpoint(&endpoint_in(dir, "work")).is_ok());
    let long = endpoint_in(dir, &"s".repeat(200));
    let err = check_endpoint(&long).unwrap_err();
    assert!(err.contains("shorter session name"), "{err}");
}
