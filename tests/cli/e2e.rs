use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mpx");

/// One isolated mpx session under a temp runtime dir; its daemon is stopped on drop.
struct Session {
    _dir: tempfile::TempDir,
    runtime: PathBuf,
    name: String,
}

impl Session {
    fn new(tag: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("mpx")
            .tempdir()
            .expect("temp dir");
        let runtime = dir.path().to_path_buf();
        let name = format!("{tag}{}", std::process::id() % 100_000);
        Self {
            _dir: dir,
            runtime,
            name,
        }
    }

    fn endpoint(&self) -> String {
        self.runtime
            .join("mpx")
            .join(format!("{}.sock", self.name))
            .to_string_lossy()
            .into_owned()
    }

    fn base(&self) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.env("XDG_RUNTIME_DIR", &self.runtime)
            .env("MPX_STATE_DIR", self.runtime.join("state"))
            .env_remove("MPX_ENDPOINT")
            .env_remove("MPX_SESSION")
            .env_remove("MPX_PANE")
            .env_remove("TMUX")
            .arg("-s")
            .arg(&self.name);
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.base().args(args).output().expect("spawn mpx")
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8_lossy(&self.run(args).stdout).into_owned()
    }

    fn code(&self, args: &[&str]) -> i32 {
        self.run(args).status.code().unwrap_or(-1)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.base().arg("kill-server").output();
    }
}

#[test]
fn list_sessions_json_reports_the_running_session() {
    let s = Session::new("ls");
    let _ = s.run(&["run", "-n", "keep", "sleep", "30"]);
    let json: serde_json::Value = serde_json::from_str(&s.stdout(&["ls", "--json"])).expect("json");
    let names: Vec<&str> = json
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|x| x["name"].as_str())
        .collect();
    assert!(names.contains(&s.name.as_str()), "{names:?}");
}

#[test]
fn list_panes_json_lists_nodes_by_name() {
    let s = Session::new("lp");
    let _ = s.run(&["run", "-n", "worker", "sleep", "30"]);
    let json: serde_json::Value =
        serde_json::from_str(&s.stdout(&["list-panes", "--json"])).expect("json");
    assert_eq!(json[0]["name"], "worker");
    assert!(json[0]["id"].as_u64().is_some());
}

#[test]
fn has_session_follows_the_daemon_lifecycle() {
    let s = Session::new("hs");
    assert_eq!(s.code(&["has-session"]), 1);
    let _ = s.run(&["run", "-n", "p", "sleep", "30"]);
    assert_eq!(s.code(&["has-session", "-t", s.name.as_str()]), 0);
    assert_eq!(s.code(&["kill-server"]), 0);
    assert_eq!(s.code(&["has-session"]), 1);
}

#[test]
fn wait_returns_the_child_exit_code() {
    let s = Session::new("wx");
    let _ = s.run(&["run", "-n", "quick", "sh", "-c", "exit 7"]);
    assert_eq!(s.code(&["wait", "quick"]), 7);
}

#[test]
fn wait_times_out_with_124() {
    let s = Session::new("wt");
    let _ = s.run(&["run", "-n", "idle", "sleep", "30"]);
    assert_eq!(s.code(&["wait", "idle", "--timeout", "1"]), 124);
}

#[test]
fn wait_for_a_screen_match_exits_zero() {
    let s = Session::new("wm");
    let _ = s.run(&[
        "run",
        "-n",
        "echoer",
        "--pty",
        "sh",
        "-c",
        "echo mpx-ready; sleep 30",
    ]);
    assert_eq!(
        s.code(&["wait", "echoer", "--match", "mpx-ready", "--timeout", "2"]),
        0
    );
}

#[test]
fn paste_round_trips_through_a_pty_program() {
    let s = Session::new("pp");
    let _ = s.run(&["run", "-n", "cat", "--pty", "cat"]);
    assert_eq!(
        s.code(&["paste", "-t", "cat", "--submit", "round-trip-token"]),
        0
    );
    assert_eq!(
        s.code(&[
            "wait",
            "cat",
            "--match",
            "round-trip-token",
            "--timeout",
            "2"
        ]),
        0
    );
}

#[test]
fn send_keys_literal_and_named_keys_reach_the_pane() {
    let s = Session::new("sk");
    let _ = s.run(&["run", "-n", "keys", "--pty", "cat"]);
    assert_eq!(s.code(&["send-keys", "-t", "keys", "-l", "typed-text"]), 0);
    assert_eq!(s.code(&["send-keys", "-t", "keys", "Enter"]), 0);
    assert_eq!(
        s.code(&["wait", "keys", "--match", "typed-text", "--timeout", "2"]),
        0
    );
}

#[test]
fn kill_server_is_safe_without_a_daemon() {
    let s = Session::new("kn");
    assert_eq!(s.code(&["kill-server"]), 1);
}

#[test]
fn attach_refuses_nested_use_inside_a_pane() {
    let s = Session::new("nest");
    let out = s
        .base()
        .env("MPX_ENDPOINT", s.endpoint())
        .arg("attach")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nested"));
}

#[test]
fn hook_outside_mpx_is_silent_and_successful() {
    let mut child = Command::new(BIN)
        .args(["hook", "claude"])
        .env_remove("MPX_PANE")
        .env_remove("MPX_ENDPOINT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"session_id":"x","hook_event_name":"Stop"}"#)
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn hook_reports_agent_state_for_the_pane() {
    let s = Session::new("hk");
    let _ = s.run(&["run", "-n", "agent", "--pty", "cat"]);
    let mut child = s
        .base()
        .args(["hook", "claude"])
        .env("MPX_PANE", "agent")
        .env("MPX_ENDPOINT", s.endpoint())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"session_id":"s-1","hook_event_name":"Notification","message":"need input"}"#,
        )
        .expect("write");
    assert_eq!(child.wait().expect("wait").code(), Some(0));
    assert_eq!(
        s.code(&["wait", "agent", "--state", "needs_input", "--timeout", "2"]),
        0
    );
}

#[test]
fn print_config_emits_claude_hook_settings() {
    let s = Session::new("pc");
    let json: serde_json::Value =
        serde_json::from_str(&s.stdout(&["hook", "print-config", "claude"])).expect("json");
    assert!(json["hooks"]["SessionStart"].is_array());
    assert!(json["hooks"]["SessionEnd"].is_array());
}

#[test]
fn tmux_multicall_splits_sends_and_captures() {
    let s = Session::new("tm");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let shim_dir = s.runtime.join("bin");
    std::fs::create_dir_all(&shim_dir).expect("bin dir");
    let link = shim_dir.join("tmux");
    std::os::unix::fs::symlink(BIN, &link).expect("symlink");
    let tmux = |args: &[&str]| {
        Command::new(&link)
            .args(args)
            .env("MPX_ENDPOINT", s.endpoint())
            .env("MPX_SESSION", &s.name)
            .env_remove("TMUX")
            .output()
            .expect("run tmux")
    };
    let split = tmux(&[
        "split-window",
        "-h",
        "-P",
        "-F",
        "#{pane_id}",
        "-t",
        "base",
        "cat",
    ]);
    assert!(
        split.status.success(),
        "{}",
        String::from_utf8_lossy(&split.stderr)
    );
    let pane = String::from_utf8_lossy(&split.stdout).trim().to_string();
    assert!(pane.starts_with('%'), "{pane}");
    assert!(
        tmux(&["send-keys", "-t", pane.as_str(), "-l", "shim-token"])
            .status
            .success()
    );
    assert!(
        tmux(&["send-keys", "-t", pane.as_str(), "Enter"])
            .status
            .success()
    );
    let captured = (0..20).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        String::from_utf8_lossy(&tmux(&["capture-pane", "-p", "-t", pane.as_str()]).stdout)
            .contains("shim-token")
    });
    assert!(captured, "capture-pane never showed the typed token");
}

#[test]
fn help_lists_restore_and_the_idempotency_flag() {
    let out = Command::new(BIN).arg("--help").output().expect("spawn");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("restore"), "{help}");
    assert!(help.contains("--idempotency-key"), "{help}");
}

#[test]
fn restore_without_a_snapshot_exits_one_with_a_message() {
    let s = Session::new("rs");
    let out = s.run(&["restore"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("snapshot"), "{stderr}");
}

#[test]
fn idempotency_key_delivers_a_repeated_send_once() {
    let s = Session::new("ik");
    let _ = s.run(&["run", "-n", "echo", "cat"]);
    for _ in 0..2 {
        assert_eq!(
            s.code(&[
                "--idempotency-key",
                "dedupe-1",
                "send",
                "echo",
                "once-token"
            ]),
            0
        );
    }
    assert_eq!(
        s.code(&["wait", "echo", "--match", "once-token", "--timeout", "2"]),
        0
    );
    std::thread::sleep(Duration::from_millis(200));
    let text = s.stdout(&["logs", "echo", "--plain"]);
    assert_eq!(text.matches("once-token").count(), 1, "{text}");
}

#[test]
fn logs_follow_prints_a_burst_without_gaps() {
    let s = Session::new("lf");
    let _ = s.run(&["run", "-n", "burst", "sh", "-c", "sleep 0.05; seq 1 2000"]);
    let mut child = s
        .base()
        .args(["logs", "burst", "-f"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn logs -f");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut lines: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while lines.last().map(String::as_str) != Some("2000") {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => lines.push(line),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let numbers: Vec<u64> = lines.iter().filter_map(|l| l.parse().ok()).collect();
    assert_eq!(numbers.first(), Some(&1), "{numbers:?}");
    assert_eq!(numbers.last(), Some(&2000), "{numbers:?}");
    assert!(
        numbers.windows(2).all(|w| w[1] == w[0] + 1),
        "gap in {numbers:?}"
    );
}
