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
fn state_dir_flag_is_where_the_daemon_keeps_its_snapshot() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["kill-server"]), 0);
    assert!(f.state_dir().exists());
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
    r.no_restore = true;
    assert_eq!(
        daemon_args(&r)[8..],
        ["--no-restore", "daemon"].map(std::ffi::OsString::from)
    );
}

#[test]
fn malformed_default_config_is_reported_on_stderr_and_defaults_apply() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bad = dir.path().join("bad.toml");
    std::fs::write(&bad, "this is = = not toml").expect("bad config");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mpx"))
        .env("MPX_CONFIG", &bad)
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("MPX_STATE_DIR", dir.path().join("state"))
        .env_remove("MPX_ENDPOINT")
        .env_remove("MPX_SESSION")
        .arg("has-session")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mpx: ignoring invalid config, using defaults: "),
        "{stderr}"
    );
    assert!(stderr.contains("bad.toml: "), "{stderr}");
    assert!(out.stdout.is_empty());
}

/// Unix only: the limit is the unix socket address length; Windows pipe names have no such limit.
#[cfg(unix)]
#[test]
fn a_session_name_too_long_for_a_socket_path_is_refused_before_any_daemon_starts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mpx"))
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("MPX_STATE_DIR", dir.path().join("state"))
        .env_remove("MPX_ENDPOINT")
        .args(["-s", &"s".repeat(200), "daemon"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("shorter session name"), "{stderr}");
}

#[test]
fn log_output_never_reaches_stdout() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mpx"))
        .env("RUST_LOG", "trace")
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("MPX_STATE_DIR", dir.path().join("state"))
        .env_remove("MPX_ENDPOINT")
        .env_remove("MPX_SESSION")
        .args(["list-sessions", "--json"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    // Windows lists the shared pipe namespace, where other tests' daemons may run: the output is
    // checked to be the JSON array alone, not to be empty.
    let listed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&out.stdout));
    });
    assert!(listed.is_array(), "{listed}");
}
