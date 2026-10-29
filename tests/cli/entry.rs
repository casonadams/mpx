use std::path::{Path, PathBuf};

use mpx::cli::{daemon_args, main_entry, resolve};

use super::fixture::{Fixture, run_command};

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn version_flag_exits_zero() {
    assert_eq!(main_entry(argv(&["mpx", "--version"])), 0);
}

#[test]
fn has_session_without_a_daemon_reports_absent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let endpoint = dir.path().join("none.sock");
    let endpoint = endpoint.to_string_lossy();
    let code = main_entry(argv(&["mpx", "--endpoint", &endpoint, "has-session"]));
    assert_eq!(code, 1);
}

#[test]
fn tmux_multicall_by_argv0_answers_version() {
    assert_eq!(main_entry(argv(&["tmux", "-V"])), 0);
}

#[test]
fn malformed_config_file_fails_before_the_command_runs() {
    let f = Fixture::start();
    let bad = f.path().join("bad.toml");
    std::fs::write(&bad, "this is = = not toml").expect("bad config");
    assert_eq!(main_entry(f.argv_with_config(&bad, &["list-panes"])), 1);
    assert_eq!(run_command(&f, &["list-panes"]), 0);
}

#[test]
fn state_dir_flag_is_where_the_daemon_keeps_its_pipes() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["pipe", "create", "p"]), 0);
    let pipes = std::fs::read_dir(f.state_dir().join("pipes"))
        .expect("pipes under the state dir")
        .count();
    assert_eq!(pipes, 1);
}

#[test]
fn auto_spawned_daemon_serves_the_same_session_endpoint_config_and_state() {
    let mut r = resolve(
        Some("t"),
        Some("/tmp/t.sock"),
        None,
        None,
        Path::new("/unused"),
    );
    r.state_dir = PathBuf::from("/state/t");
    let plain = daemon_args(&r);
    assert_eq!(
        plain,
        [
            "--session",
            "t",
            "--endpoint",
            "/tmp/t.sock",
            "--state-dir",
            "/state/t",
            "daemon"
        ]
        .map(std::ffi::OsString::from)
    );
    r.config_path = Some(PathBuf::from("/cfg/mpx.toml"));
    let with_config = daemon_args(&r);
    assert_eq!(
        with_config[6..],
        ["-f", "/cfg/mpx.toml", "daemon"].map(std::ffi::OsString::from)
    );
}
