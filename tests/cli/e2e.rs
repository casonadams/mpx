use std::io::{BufRead, BufReader};
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
        .env("XDG_RUNTIME_DIR", &s.runtime)
        .env("MPX_STATE_DIR", s.runtime.join("state"))
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

#[cfg(unix)]
#[test]
fn tmux_multicall_keeps_options_buffers_and_runs_command_strings() {
    let s = Session::new("tx");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));

    assert!(t(&["set", "-g", "@theme", "dark mode"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "@theme"]), "dark mode");
    assert_eq!(out(&["show-options", "-g"]), "@theme \"dark mode\"");
    assert!(!t(&["show-options", "-gv", "@nope"]).status.success());
    assert!(t(&["show-options", "-gqv", "@nope"]).status.success());
    assert!(t(&["set-option", "-gu", "@theme"]).status.success());
    assert_eq!(out(&["show-options", "-g"]), "");

    assert!(t(&["set-buffer", "-b", "one", "first"]).status.success());
    assert!(t(&["set-buffer", "second"]).status.success());
    assert_eq!(out(&["show-buffer", "-b", "one"]), "first");
    assert_eq!(
        out(&["list-buffers", "-F", "#{buffer_name}:#{buffer_size}"]),
        "buffer0:6\none:5"
    );

    assert!(t(&["wait-for", "-S", "go"]).status.success());
    assert!(t(&["wait-for", "go"]).status.success());

    assert_eq!(out(&["run-shell", "echo from-shell"]), "from-shell");
    let failing = t(&["run-shell", "exit 3"]);
    assert!(!failing.status.success());
    assert!(String::from_utf8_lossy(&failing.stderr).contains("returned 3"));

    assert!(
        t(&["if-shell", "false", "set -g @branch yes ; set -g @both 1", "set -g @branch no"])
            .status
            .success()
    );
    assert_eq!(out(&["show-options", "-gv", "@branch"]), "no");
    assert!(t(&["if-shell", "-F", "#{==:a,a}", "set -g @branch yes"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "@branch"]), "yes");

    let conf = s.runtime.join("c.conf");
    std::fs::write(&conf, "set -g @a 1\n# note\nset -g @b \"x y\"\nnosuchcommand\n").unwrap();
    let sourced = t(&["source-file", conf.to_str().unwrap()]);
    assert!(!sourced.status.success());
    assert!(String::from_utf8_lossy(&sourced.stderr).contains("c.conf:4: unknown command"));
    assert_eq!(out(&["show-options", "-gv", "@b"]), "x y");
}

#[cfg(unix)]
#[test]
fn tmux_multicall_creates_sessions_and_servers_and_kills_them_all() {
    let s = Session::new("tn");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));
    let extra = format!("{}x", s.name);

    assert!(t(&["new-session", "-d", "-s", &extra, "-n", "w", "cat"]).status.success());
    let sessions = out(&["list-sessions", "-F", "#{session_name}"]);
    assert!(sessions.lines().any(|l| l == extra), "{sessions}");
    let duplicate = t(&["new-session", "-d", "-s", &extra]);
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate session"));
    assert!(t(&["has-session", "-t", &extra]).status.success());

    assert!(t(&["-L", "other", "new-session", "-d", "-s", "inner"]).status.success());
    assert_eq!(out(&["-L", "other", "list-sessions", "-F", "#{session_name}"]), "inner");
    let default_server = out(&["list-sessions", "-F", "#{session_name}"]);
    assert!(!default_server.lines().any(|l| l == "inner"), "{default_server}");

    assert!(t(&["-L", "other", "kill-server"]).status.success());
    assert!(!t(&["-L", "other", "list-sessions"]).status.success());
    assert!(t(&["kill-server"]).status.success());
    assert!(!t(&["list-sessions"]).status.success());
}

#[cfg(unix)]
#[test]
fn tmux_multicall_pastes_buffers_files_environment_windows_and_pipes() {
    let s = Session::new("tp");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));
    let ok = |args: &[&str]| {
        let run = t(args);
        assert!(run.status.success(), "{args:?}: {}", String::from_utf8_lossy(&run.stderr));
    };

    ok(&["set-environment", "MPX_E2E", "v1"]);
    assert_eq!(out(&["show-environment"]), "MPX_E2E=v1");
    assert_eq!(out(&["show-environment", "MPX_E2E"]), "MPX_E2E=v1");
    ok(&["set-environment", "-u", "MPX_E2E"]);
    assert!(!t(&["show-environment", "MPX_E2E"]).status.success());

    let file = s.runtime.join("buf.txt");
    let file = file.to_str().unwrap();
    ok(&["set-buffer", "-b", "keep", "saved text"]);
    ok(&["save-buffer", "-b", "keep", file]);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "saved text");
    ok(&["save-buffer", "-a", "-b", "keep", file]);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "saved textsaved text");
    ok(&["load-buffer", "-b", "loaded", file]);
    assert_eq!(out(&["show-buffer", "-b", "loaded"]), "saved textsaved text");
    ok(&["delete-buffer", "-b", "loaded"]);
    assert!(!t(&["show-buffer", "-b", "loaded"]).status.success());

    ok(&["set-buffer", "-b", "p", "pasted-marker"]);
    ok(&["paste-buffer", "-d", "-b", "p", "-t", "base"]);
    assert!(!t(&["show-buffer", "-b", "p"]).status.success(), "-d deletes it");
    let seen = (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        out(&["capture-pane", "-p", "-t", "base"]).contains("pasted-marker")
    });
    assert!(seen, "the paste never reached the pane");

    ok(&["detach-client"]);
    ok(&["switch-client", "-n"]);
    assert!(!t(&["switch-client"]).status.success(), "a target is required");

    ok(&["new-window", "-d", "-n", "second", "cat"]);
    let names = |_: ()| out(&["list-windows", "-F", "#{window_index}:#{window_name}"]);
    assert_eq!(names(()).lines().count(), 2);
    let first = names(()).lines().next().unwrap().to_string();
    ok(&["swap-window", "-s", "0", "-t", "1"]);
    assert_ne!(names(()).lines().next().unwrap(), first);
    ok(&["move-window", "-s", "1", "-t", "0"]);
    assert_eq!(names(()).lines().next().unwrap(), first, "moving window 1 back to 0");

    ok(&["select-window", "-n"]);
    ok(&["select-window", "-p"]);
    ok(&["select-window", "-l"]);
    ok(&["next-window"]);
    ok(&["previous-window"]);
    ok(&["select-window", "-t", "0"]);
    ok(&["swap-window", "-t", "1"]);
    ok(&["swap-window", "-t", "0"]);
    ok(&["run-shell", "-b", "true"]);

    let pane = out(&["split-window", "-d", "-P", "-F", "#{pane_id}", "cat"]);
    ok(&["join-pane", "-s", &pane, "-t", "base", "-h"]);
    assert!(!t(&["join-pane", "-s", "base", "-t", "base"]).status.success());

    let piped = s.runtime.join("piped.txt");
    let command = format!("cat > {}", piped.display());
    ok(&["pipe-pane", "-t", "base", &command]);
    ok(&["send-keys", "-t", "base", "-l", "to-the-pipe"]);
    let got = (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(&piped).is_ok_and(|text| text.contains("to-the-pipe"))
    });
    assert!(got, "pipe-pane never delivered output");
    assert!(!t(&["pipe-pane", "-I", "-t", "base", "cat"]).status.success());
}

#[cfg(unix)]
#[test]
fn tmux_multicall_attach_needs_a_terminal_and_names_the_session_it_attaches_to() {
    let s = Session::new("ta");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    tmux(&s, &["has-session"]);
    let link = s.runtime.join("bin").join("tmux");
    let attach = Command::new(&link)
        .args(["attach-session", "-t", &s.name])
        .env("XDG_RUNTIME_DIR", &s.runtime)
        .env("MPX_STATE_DIR", s.runtime.join("state"))
        .env_remove("MPX_ENDPOINT")
        .env_remove("MPX_SESSION")
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .output()
        .expect("run tmux");
    assert!(!attach.status.success());
    assert!(
        String::from_utf8_lossy(&attach.stderr).contains("raw mode"),
        "attach did not reach the terminal UI: {}",
        String::from_utf8_lossy(&attach.stderr)
    );
}

#[cfg(unix)]
#[test]
fn tmux_multicall_binds_keys_sets_hooks_and_sources_conditional_files() {
    let s = Session::new("tk");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));
    let ok = |args: &[&str]| {
        let run = t(args);
        assert!(run.status.success(), "{args:?}: {}", String::from_utf8_lossy(&run.stderr));
    };

    let defaults = out(&["list-keys"]);
    assert!(defaults.contains("bind-key -T prefix x kill-pane"), "{defaults}");
    ok(&["bind-key", "y", "split-window", "-h", ";", "select-layout", "tiled"]);
    ok(&["bind", "-n", "M-h", "select-pane", "-L"]);
    ok(&["unbind-key", "x"]);
    let keys = out(&["list-keys"]);
    assert!(keys.contains("bind-key -T prefix y split-window -h ; select-layout tiled"), "{keys}");
    assert!(keys.contains("bind-key -T root M-h select-pane -L"), "{keys}");
    assert!(!keys.contains("-T prefix x "), "{keys}");
    assert_eq!(out(&["list-keys", "-T", "root"]).lines().count(), 2, "C-Space and M-h");

    let bad = t(&["bind-key", "q", "nosuchcommand"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown command"));
    let table = t(&["bind-key", "-T", "copy-mode-vi", "v", "send-keys", "x"]);
    assert!(String::from_utf8_lossy(&table.stderr).contains("send-keys -X"));
    ok(&["bind-key", "-T", "copy-mode-vi", "v", "send-keys", "-X", "cancel"]);
    assert!(out(&["list-keys", "-T", "copy-mode-vi"]).contains("send-keys -X cancel"));
    ok(&["unbind-key", "-T", "copy-mode-vi", "v"]);
    ok(&["unbind-key", "-a", "-n"]);
    assert_eq!(out(&["list-keys", "-T", "root"]), "");

    let mark = s.runtime.join("hook.txt");
    let command = format!("run-shell 'touch {}'", mark.display());
    ok(&["set-hook", "-g", "after-new-window", &command]);
    assert!(out(&["show-hooks"]).starts_with("after-new-window "));
    assert!(!t(&["set-hook", "after-nonsense", "x"]).status.success());
    ok(&["new-window", "-d", "cat"]);
    assert!(
        (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            mark.exists()
        }),
        "after-new-window never ran"
    );
    ok(&["set-hook", "-gu", "after-new-window"]);
    assert_eq!(out(&["show-hooks"]), "");

    let conf = s.runtime.join("cond.conf");
    std::fs::write(
        &conf,
        "set -g @a base\n%if \"#{==:1,1}\"\nset -g @b yes\n%if 0\nset -g @c never\n%endif\n%else\nset -g @d never\n%endif\n%if 0\nset -g @e never\n%elif 1\nset -g @f elif\n%else\nset -g @g never\n%endif\n",
    )
    .unwrap();
    ok(&["source-file", conf.to_str().unwrap()]);
    let set = out(&["show-options", "-g"]);
    for want in ["@a base", "@b yes", "@f elif"] {
        assert!(set.contains(want), "{want} missing in {set}");
    }
    for never in ["@c", "@d", "@e", "@g"] {
        assert!(!set.contains(never), "{never} ran in {set}");
    }
    std::fs::write(&conf, "%if 1\nset -g @x 1\n").unwrap();
    let open = t(&["source-file", conf.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&open.stderr).contains("%if without %endif"));
}

#[test]
fn tmux_multicall_rotates_respawns_and_finds_windows() {
    let s = Session::new("tw");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let split = tmux(
        &s,
        &["split-window", "-h", "-l", "25", "-d", "-t", "base", "cat"],
    );
    assert!(
        split.status.success(),
        "{}",
        String::from_utf8_lossy(&split.stderr)
    );
    let width = |s: &Session| -> u16 {
        let out = tmux(s, &["display-message", "-p", "-t", "base", "#{pane_width}"]);
        stdout(&out).parse().expect("pane width")
    };

    // The wide pane starts in the left slot. Rotating up hands base the narrow slot, and
    // rotating down gives the wide one back.
    let wide = width(&s);
    assert!(wide > 40, "base should start as the wide pane: {wide}");
    assert!(tmux(&s, &["rotate-window"]).status.success());
    let narrow = width(&s);
    assert!(narrow < wide, "rotated base is {narrow}, was {wide}");
    assert!(tmux(&s, &["rotate-window", "-D"]).status.success());
    assert_eq!(width(&s), wide);

    let restart = tmux(
        &s,
        &["respawn-pane", "-k", "-t", "base", "echo respawned; cat"],
    );
    assert!(
        restart.status.success(),
        "{}",
        String::from_utf8_lossy(&restart.stderr)
    );
    let printed = (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        stdout(&tmux(&s, &["capture-pane", "-p", "-t", "base"])).contains("respawned")
    });
    assert!(printed, "the respawned command never printed");
    assert_eq!(width(&s), wide, "respawn keeps the pane's slot");

    let busy = tmux(&s, &["respawn-pane", "-t", "base", "cat"]);
    assert!(!busy.status.success());
    assert!(String::from_utf8_lossy(&busy.stderr).contains("still active"));

    let found = tmux(&s, &["find-window", "-C", "respawned"]);
    assert!(
        found.status.success(),
        "{}",
        String::from_utf8_lossy(&found.stderr)
    );
    assert!(stdout(&found).starts_with("0:"), "{}", stdout(&found));
    let by_name = tmux(&s, &["find-window", "-N", "respawned"]);
    assert!(!by_name.status.success());
    assert!(String::from_utf8_lossy(&by_name.stderr).contains("no windows matching: respawned"));
}

#[cfg(unix)]
#[test]
fn tmux_multicall_numbers_panes_per_window_and_steps_to_windows_with_alerts() {
    let s = Session::new("tw");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));

    assert!(t(&["set", "-g", "monitor-activity", "on"]).status.success());
    assert!(!t(&["next-window", "-a"]).status.success(), "no other window to alert");
    assert!(t(&["new-window", "-d", "-n", "noisy", "sh", "-c", "sleep 0.3; echo hi; sleep 30"]).status.success());
    assert!(t(&["new-window", "-d", "-n", "quiet", "cat"]).status.success());
    let flagged = (0..60).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        out(&["list-windows", "-F", "#{window_name}:#{window_flags}"]).contains("noisy:#")
    });
    assert!(flagged, "output in a background window sets the activity flag");
    assert!(t(&["next-window", "-a"]).status.success());
    assert_eq!(out(&["display-message", "-p", "#{window_name}"]), "noisy");
    assert!(!t(&["next-window", "-a"]).status.success(), "the alert was cleared by visiting");

    assert!(t(&["split-window", "-d", "cat"]).status.success());
    let indexes = out(&["list-panes", "-a", "-F", "#{window_name}:#{pane_index}"]);
    let lines: Vec<&str> = indexes.lines().collect();
    assert!(lines.contains(&"quiet:0") && lines.contains(&"noisy:0"), "{indexes}");
    assert!(lines.iter().any(|l| l.ends_with(":1")), "second pane of a window is 1: {indexes}");
}

#[cfg(unix)]
#[test]
fn tmux_multicall_validates_options_and_reports_synchronized_panes() {
    let s = Session::new("to");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args));

    let refused = t(&["set", "-g", "remain-on-exit", "maybe"]);
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("invalid value: maybe"), "{stderr}");
    assert!(!t(&["set", "-g", "monitor-silence", "5"]).status.success());
    assert!(t(&["set", "-g", "remain-on-exit", "failed"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "remain-on-exit"]), "failed");
    assert_eq!(out(&["show-options", "-gv", "history-limit"]), "100000");

    assert!(t(&["setw", "-t", "0", "synchronize-panes", "on"]).status.success());
    let flags = &["list-windows", "-F", "#{pane_synchronized}"];
    assert_eq!(out(flags), "1");
    assert!(t(&["setw", "synchronize-panes", "off"]).status.success());
    assert_eq!(out(flags), "0");
}

#[test]
fn tmux_multicall_numbers_windows_from_base_index_and_reports_ui_options() {
    let s = Session::new("bidx");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let out = |args: &[&str]| stdout(&t(args)).trim().to_string();
    assert_eq!(out(&["show-options", "-gv", "base-index"]), "0");
    assert!(t(&["set", "-g", "base-index", "1"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "base-index"]), "1");
    assert_eq!(out(&["display-message", "-p", "#{window_index}"]), "1");
    assert!(t(&["select-window", "-t", "1"]).status.success());
    assert!(
        !t(&["select-window", "-t", "0"]).status.success(),
        "window 0 does not exist with base-index 1"
    );
    let refused = t(&["set", "-g", "renumber-windows", "off"]);
    assert!(!refused.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("windows are always renumbered"), "{said}");
    assert_eq!(out(&["show-options", "-gv", "renumber-windows"]), "on");
    assert_eq!(out(&["show-options", "-gv", "mouse"]), "on");
    assert!(t(&["set", "-g", "mouse", "off"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "mouse"]), "off");
    assert!(t(&["set", "-g", "escape-time", "0"]).status.success());
    assert_eq!(out(&["show-options", "-gv", "escape-time"]), "0");
}

/// `display-popup -E` from a script blocks until the popup's program exits, and exits with its status.
#[cfg(unix)]
#[test]
fn display_popup_with_e_blocks_until_the_program_exits_and_returns_its_status() {
    let s = Session::new("dp");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let started = std::time::Instant::now();
    let out = tmux(&s, &["display-popup", "-E", "sleep 1; exit 3"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "the call waited for the popup to close"
    );
}

/// Without -E the call returns at once; the popup stays until `display-popup -C` closes it.
#[cfg(unix)]
#[test]
fn display_popup_without_e_returns_at_once_and_c_closes_it() {
    let s = Session::new("dq");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let started = std::time::Instant::now();
    let opened = tmux(&s, &["display-popup", "sleep 30"]);
    assert_eq!(opened.status.code(), Some(0));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let closed = tmux(&s, &["display-popup", "-C"]);
    assert_eq!(closed.status.code(), Some(0));
}

#[test]
fn tmux_multicall_zooms_with_resize_pane_and_reports_it_in_formats() {
    let s = Session::new("tz");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    assert!(
        t(&["split-window", "-d", "-t", "base", "cat"])
            .status
            .success()
    );
    let flag = |args: &[&str]| stdout(&tmux(&s, args));
    assert_eq!(flag(&["display-message", "-p", "#{window_zoomed_flag}"]), "0");
    assert_eq!(flag(&["display-message", "-p", "#{window_flags}"]), "*");

    let mark = s.runtime.join("zoomed.txt");
    let command = format!("run-shell 'touch {}'", mark.display());
    assert!(
        t(&["set-hook", "-g", "after-resize-pane", &command])
            .status
            .success()
    );
    assert!(t(&["resize-pane", "-Z", "-t", "base"]).status.success());
    assert_eq!(flag(&["display-message", "-p", "#{window_zoomed_flag}"]), "1");
    assert_eq!(flag(&["display-message", "-p", "#{window_flags}"]), "*Z");
    assert!(
        (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            mark.exists()
        }),
        "after-resize-pane never ran on the toggle"
    );

    assert!(t(&["resize-pane", "-Z"]).status.success());
    assert_eq!(flag(&["display-message", "-p", "#{window_zoomed_flag}"]), "0");
    assert_eq!(flag(&["display-message", "-p", "#{window_flags}"]), "*");
    assert!(
        t(&["set-hook", "-gu", "after-resize-pane"])
            .status
            .success()
    );
}

#[test]
fn tmux_multicall_copy_mode_commands_and_key_tables_are_checked_before_use() {
    let s = Session::new("cm");
    let _ = s.run(&["run", "-n", "base", "--pty", "cat"]);
    let t = |args: &[&str]| tmux(&s, args);
    let err = |args: &[&str]| String::from_utf8_lossy(&t(args).stderr).into_owned();

    // No terminal is attached: copy-mode reaches nobody, and that is not an error.
    assert!(t(&["copy-mode"]).status.success());
    assert!(t(&["copy-mode", "-e", "-u"]).status.success());
    assert!(t(&["copy-mode", "-q"]).status.success());

    // Commands need the pane in a mode; their names and arguments are checked first.
    assert!(err(&["send-keys", "-t", "base", "-X", "cancel"]).contains("pane %0 not in a mode"));
    assert!(err(&["send-keys", "-X", "copy-selection-and-cancel"]).contains("not in a mode"));
    assert!(err(&["send-keys", "-X", "no-such-command"]).contains("copy-mode command not supported: no-such-command"));
    assert!(err(&["send-keys", "-X"]).contains("needs a copy-mode command"));
    assert!(err(&["send-keys", "-X", "-N", "0", "cancel"]).contains("invalid repeat count '0'"));
    assert!(err(&["send-keys", "-X", "goto-line", "x"]).contains("invalid line 'x'"));
    assert!(err(&["send-keys", "-X", "search-forward"]).contains("needs an argument"));
    assert!(err(&["send-keys", "-N", "2", "cancel"]).contains("-N needs -X"));

    // Key tables take only copy commands, and list what they bind.
    assert!(t(&["bind-key", "-T", "copy-mode-vi", "C-x", "send-keys", "-X", "rectangle-toggle"]).status.success());
    assert!(err(&["bind-key", "-T", "copy-mode-vi", "K", "send-keys", "-X", "bogus"]).contains("copy-mode command not supported: bogus"));
    assert!(err(&["bind-key", "-T", "copy-mode", "K", "send-keys", "x"]).contains("send-keys -X"));
    let vi = stdout(&t(&["list-keys", "-T", "copy-mode-vi"]));
    assert!(vi.contains("bind-key -T copy-mode-vi C-x send-keys -X rectangle-toggle"), "{vi}");
    assert!(vi.contains("bind-key -T copy-mode-vi C-v send-keys -X rectangle-toggle"), "{vi}");
    let emacs = stdout(&t(&["list-keys", "-T", "copy-mode"]));
    assert!(emacs.contains("bind-key -T copy-mode C-g send-keys -X cancel"), "{emacs}");
    assert!(!emacs.contains("copy-mode-vi"), "{emacs}");
    assert!(t(&["unbind-key", "-T", "copy-mode-vi", "C-x"]).status.success());
    assert!(!stdout(&t(&["list-keys", "-T", "copy-mode-vi"])).contains("C-x"));
}
