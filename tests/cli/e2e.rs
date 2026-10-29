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

/// Asserts that `out` ended with `code` and said `needle` on stderr.
fn assert_fails(out: &Output, code: i32, needle: &str) {
    assert_eq!(out.status.code(), Some(code));
    let stderr = mpx::buffer::strip_ansi(&out.stderr);
    assert!(stderr.contains(needle), "{stderr}");
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
    assert_fails(&out, 1, "nested");
}

/// Run as a child process: clap prints usage errors straight to stderr, which the test harness
/// does not capture from an in-process call.
#[test]
fn unknown_flag_is_a_usage_error() {
    let out = Command::new(BIN).arg("--no-such-flag").output().expect("spawn");
    assert_fails(&out, 2, "unexpected argument '--no-such-flag'");
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

/// Runs `mpx hook claude` as pane `pane` with `payload` on stdin; returns its stdout.
fn hook_as(s: &Session, pane: &str, payload: &serde_json::Value) -> String {
    let mut child = s
        .base()
        .args(["hook", "claude"])
        .env("MPX_PANE", pane)
        .env("MPX_ENDPOINT", s.endpoint())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn hooks_share_an_agents_task_and_edits_with_its_peers() {
    let s = Session::new("aw");
    let repo = s.runtime.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("git dir");
    let cwd = repo.to_string_lossy().into_owned();
    for pane in ["a", "b"] {
        let _ = s.run(&["run", "-n", pane, "--pty", "--cwd", &cwd, "cat"]);
    }
    let file = repo.join("lib.rs").to_string_lossy().into_owned();
    let event = |name: &str, sid: &str, extra: serde_json::Value| {
        let mut v = serde_json::json!({ "hook_event_name": name, "session_id": sid, "cwd": cwd });
        if let (Some(obj), Some(more)) = (v.as_object_mut(), extra.as_object()) {
            obj.extend(more.clone());
        }
        v
    };
    let prompt = event("UserPromptSubmit", "sa", serde_json::json!({ "prompt": "split the parser" }));
    assert_eq!(hook_as(&s, "a", &prompt), "", "a has no peers yet");
    let edit = serde_json::json!({ "tool_name": "Write", "tool_input": { "file_path": file } });
    let _ = hook_as(&s, "a", &event("PostToolUse", "sa", edit));
    let roster = hook_as(&s, "b", &event("SessionStart", "sb", serde_json::json!({})));
    assert!(roster.contains("split the parser"), "b learns what a works on: {roster}");

    let read = serde_json::json!({ "tool_name": "Read", "tool_input": { "file_path": file } });
    let out: serde_json::Value =
        serde_json::from_str(&hook_as(&s, "b", &event("PostToolUse", "sb", read))).expect("json");
    let context = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("");
    assert!(context.contains(&format!("{}:a", s.name)), "{context}");
    assert!(context.contains("split the parser"), "{context}");
}

#[test]
fn mcp_peers_from_an_agent_pane_lists_the_other_agents_in_its_repository() {
    let s = Session::new("mp");
    let repo = s.runtime.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("git dir");
    let cwd = repo.to_string_lossy().into_owned();
    for pane in ["a", "b"] {
        let _ = s.run(&["run", "-n", pane, "--pty", "--cwd", &cwd, "cat"]);
        let prompt = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": format!("s-{pane}"),
            "cwd": cwd,
            "prompt": format!("task of {pane}"),
        });
        let _ = hook_as(&s, pane, &prompt);
    }
    let mut child = s
        .base()
        .arg("mcp")
        .current_dir(&repo)
        .env("MPX_PANE", "b")
        .env("MPX_ENDPOINT", s.endpoint())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp");
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "mpx_peers", "arguments": {} }
    });
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(format!("{call}\n").as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).expect("reply");
    let text = reply["result"]["content"][0]["text"].as_str().expect("text");
    let peers: serde_json::Value = serde_json::from_str(text).expect("peers json");
    let agents: Vec<&str> = peers["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .filter_map(|a| a["agent"].as_str())
        .collect();
    assert_eq!(agents, vec![format!("{}:a", s.name)], "{peers}");
    assert_eq!(peers["agents"][0]["task"], "task of a");
}

#[test]
fn print_config_emits_claude_hook_settings() {
    let s = Session::new("pc");
    let json: serde_json::Value =
        serde_json::from_str(&s.stdout(&["hook", "print-config", "claude"])).expect("json");
    assert!(json["hooks"]["SessionStart"].is_array());
    assert!(json["hooks"]["SessionEnd"].is_array());
}

/// Runs the binary as `tmux` (argv0 multicall) against `s`, creating the symlink on first use.
fn tmux(s: &Session, args: &[&str]) -> Output {
    let shim_dir = s.runtime.join("bin");
    let link = shim_dir.join(if cfg!(windows) { "tmux.exe" } else { "tmux" });
    if !link.exists() {
        std::fs::create_dir_all(&shim_dir).expect("bin dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(BIN, &link).expect("symlink");
        #[cfg(windows)]
        std::fs::copy(BIN, &link).expect("copy tmux.exe");
    }
    Command::new(&link)
        .args(args)
        .env("MPX_ENDPOINT", s.endpoint())
        .env("MPX_SESSION", &s.name)
        .env_remove("TMUX")
        .output()
        .expect("run tmux")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn tmux_multicall_lists_resizes_and_lays_out_panes() {
    let s = Session::new("tl");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    assert!(tmux(&s, &["has-session"]).status.success());
    assert!(!tmux(&s, &["has-session", "-t", "nosuch"]).status.success());

    let watcher = "trap 'stty size' WINCH; while :; do sleep 0.05; done";
    let split = tmux(
        &s,
        &[
            "split-window",
            "-h",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            watcher,
        ],
    );
    let pane = stdout(&split);
    let panes = stdout(&tmux(&s, &["list-panes", "-F", "#{pane_id}"]));
    assert_eq!(panes.lines().count(), 2, "{panes}");
    assert!(panes.lines().any(|p| p == pane), "{panes}");

    assert!(
        tmux(&s, &["resize-pane", "-t", &pane, "-x", "30"])
            .status
            .success()
    );
    let resized = (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        stdout(&tmux(&s, &["capture-pane", "-p", "-t", &pane])).contains(" 30")
    });
    assert!(resized, "the pane program never saw 30 columns");

    let widths = |s: &Session| stdout(&tmux(s, &["list-panes", "-F", "#{pane_width}"]));
    let side_by_side = widths(&s);
    assert!(
        tmux(&s, &["select-layout", "even-vertical"])
            .status
            .success()
    );
    let stacked = widths(&s);
    assert_ne!(stacked, side_by_side);
    assert_eq!(
        stacked
            .lines()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        1,
        "{stacked}"
    );

    let bogus = tmux(&s, &["select-layout", "bogus"]);
    assert!(!bogus.status.success());
    assert!(String::from_utf8_lossy(&bogus.stderr).contains("unknown layout"));
}

#[test]
fn tmux_multicall_splits_sends_and_captures() {
    let s = Session::new("tm");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let tmux = |args: &[&str]| tmux(&s, args);
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

    let window = tmux(&[
        "new-window",
        "-d",
        "-n",
        "side",
        "-P",
        "-F",
        "#{window_index}:#{window_name}",
        "cat",
    ]);
    assert!(
        window.status.success(),
        "{}",
        String::from_utf8_lossy(&window.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&window.stdout).trim(), "1:side");
    let windows = tmux(&["list-windows", "-F", "#{window_index}:#{window_name}"]);
    let windows = String::from_utf8_lossy(&windows.stdout).into_owned();
    assert_eq!(windows.lines().count(), 2, "{windows}");
    assert!(
        windows.lines().any(|w| w == "1:side"),
        "-n names the window even while cat runs in it: {windows}"
    );
    let active = tmux(&["display-message", "-p", "#{window_index}"]);
    assert_eq!(
        String::from_utf8_lossy(&active.stdout).trim(),
        "0",
        "-d keeps the current window active"
    );
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
    assert_fails(&s.run(&["restore"]), 1, "snapshot");
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

#[cfg(unix)]
#[test]
fn an_auto_started_daemon_leads_its_own_session_so_terminal_hangups_spare_it() {
    let s = Session::new("sid");
    let _ = s.run(&["run", "-n", "a", "--pty", "cat"]);
    let sessions: serde_json::Value =
        serde_json::from_str(&s.stdout(&["list-sessions", "--json"])).expect("json");
    let pid = sessions[0]["pid"].as_i64().expect("daemon pid") as libc::pid_t;
    // SAFETY: getsid only reads the session id of a process.
    let session = unsafe { libc::getsid(pid) };
    assert_eq!(session, pid, "the daemon must not share the spawning terminal's session");
}
