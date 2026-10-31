use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mpx");
const READY: &str = "mpx-ready";

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

    fn spawn(&self, name: &str, cmd: &[&str]) {
        let mut args = vec!["run", "-n", name];
        args.extend_from_slice(cmd);
        let _ = self.run(&args);
    }

    /// Starts `body` in `sh -c` as a pty pane and waits until it has printed READY.
    fn spawn_pty(&self, name: &str, body: &str) {
        let script = format!("echo {READY}; {body}");
        let _ = self.run(&["run", "-n", name, "--pty", "sh", "-c", script.as_str()]);
        assert_eq!(self.wait_for_match(name, READY), 0, "{name} never printed {READY}");
    }

    fn wait_for_match(&self, name: &str, needle: &str) -> i32 {
        self.code(&["wait", name, "--match", needle, "--timeout", "2"])
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.base().arg("kill-server").output();
    }
}

fn cat_session(tag: &str, pane: &str) -> Session {
    let s = Session::new(tag);
    s.spawn_pty(pane, "exec cat");
    s
}

/// Starts `cmd` as pane `pane` and returns the exit code of `mpx` run with `wait`.
fn wait_exit(tag: &str, pane: &str, cmd: &[&str], wait: &[&str]) -> i32 {
    let s = Session::new(tag);
    s.spawn(pane, cmd);
    s.code(wait)
}

#[test]
fn list_sessions_json_reports_the_running_session() {
    let s = Session::new("ls");
    s.spawn("keep", &["sleep", "30"]);
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
    s.spawn("worker", &["sleep", "30"]);
    let json: serde_json::Value =
        serde_json::from_str(&s.stdout(&["list-panes", "--json"])).expect("json");
    assert_eq!(json[0]["name"], "worker");
    assert!(json[0]["id"].as_u64().is_some());
}

#[test]
fn has_session_follows_the_daemon_lifecycle() {
    let s = Session::new("hs");
    assert_eq!(s.code(&["has-session"]), 1);
    s.spawn("p", &["sleep", "30"]);
    assert_eq!(s.code(&["has-session", "-t", s.name.as_str()]), 0);
    assert_eq!(s.code(&["kill-server"]), 0);
    assert_eq!(s.code(&["has-session"]), 1);
}

#[test]
fn wait_returns_the_child_exit_code() {
    assert_eq!(wait_exit("wx", "quick", &["sh", "-c", "exit 7"], &["wait", "quick"]), 7);
}

#[test]
fn wait_times_out_with_124() {
    let wait = ["wait", "idle", "--timeout", "1"];
    assert_eq!(wait_exit("wt", "idle", &["sleep", "30"], &wait), 124);
}

#[test]
fn wait_for_a_screen_match_exits_zero() {
    let s = Session::new("wm");
    s.spawn_pty("echoer", "sleep 30");
    assert_eq!(s.wait_for_match("echoer", READY), 0);
}

#[test]
fn paste_round_trips_through_a_pty_program() {
    let s = cat_session("pp", "cat");
    assert_eq!(s.code(&["paste", "-t", "cat", "--submit", "round-trip-token"]), 0);
    assert_eq!(s.wait_for_match("cat", "round-trip-token"), 0);
}

#[test]
fn send_keys_literal_and_named_keys_reach_the_pane() {
    let s = cat_session("sk", "keys");
    assert_eq!(s.code(&["send-keys", "-t", "keys", "-l", "typed-text"]), 0);
    assert_eq!(s.code(&["send-keys", "-t", "keys", "Enter"]), 0);
    assert_eq!(s.wait_for_match("keys", "typed-text"), 0);
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
    s.spawn("echo", &["cat"]);
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
    assert_eq!(s.wait_for_match("echo", "once-token"), 0);
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
    let s = cat_session("sid", "a");
    let sessions: serde_json::Value =
        serde_json::from_str(&s.stdout(&["list-sessions", "--json"])).expect("json");
    let pid = sessions[0]["pid"].as_i64().expect("daemon pid") as libc::pid_t;
    // SAFETY: getsid only reads the session id of a process.
    let session = unsafe { libc::getsid(pid) };
    assert_eq!(session, pid, "the daemon must not share the spawning terminal's session");
}