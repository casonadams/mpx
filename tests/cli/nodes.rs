use mpx::protocol::NodeState;

use super::fixture::{Fixture, run_command};

#[test]
fn run_names_the_node_and_applies_cwd_and_env() {
    let f = Fixture::start();
    std::fs::write(f.path().join("note.txt"), "from the cwd").expect("note");
    let cwd = f.path().to_string_lossy().into_owned();
    let script = "cat note.txt; echo $GREETING";
    assert_eq!(
        run_command(
            &f,
            &[
                "run",
                "-n",
                "job",
                "--env",
                "GREETING=hi",
                "--cwd",
                &cwd,
                "sh",
                "-c",
                script,
            ],
        ),
        0
    );
    assert_eq!(run_command(&f, &["wait", "job", "--timeout", "5"]), 0);
    let output = f.logs("job");
    assert!(output.contains("from the cwd"), "{output:?}");
    assert!(output.contains("hi"), "{output:?}");
}

#[test]
fn run_pty_and_remain_keep_the_node_with_its_exit_code() {
    let f = Fixture::start();
    assert_eq!(
        run_command(
            &f,
            &["run", "-n", "p", "--pty", "--remain", "sh", "-c", "exit 3"]
        ),
        0
    );
    assert_eq!(run_command(&f, &["wait", "p", "--timeout", "5"]), 3);
    let node = f.node("p");
    assert!(node.pty);
    assert_eq!(node.exit_code, Some(3));
}

#[test]
fn run_rejects_a_malformed_env_entry_without_starting_anything() {
    let f = Fixture::start();
    let args = ["run", "-n", "bad", "--env", "NOEQUALS", "true"];
    assert_eq!(run_command(&f, &args), 1);
    assert!(f.nodes().is_empty());
}

#[test]
fn send_delivers_a_line_to_the_node_input() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "echo", "cat"]), 0);
    assert_eq!(run_command(&f, &["send", "echo", "hello"]), 0);
    let wait = ["wait", "echo", "--match", "hello", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    assert_eq!(run_command(&f, &["send", "missing", "x"]), 1);
}

#[test]
fn send_keys_reaches_the_pane_by_name_and_by_pane_id() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "keys", "cat"]), 0);
    let typed = ["send-keys", "-t", "keys", "-l", "typed"];
    assert_eq!(run_command(&f, &typed), 0);
    let wait = ["wait", "keys", "--match", "typed", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    let pane = format!("%{}", f.node("keys").id);
    assert_eq!(
        run_command(&f, &["send-keys", "-t", &pane, "-l", "by-id"]),
        0
    );
    let wait = ["wait", "keys", "--match", "by-id", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    assert_eq!(
        run_command(&f, &["send-keys", "-t", "%99999", "-l", "x"]),
        1
    );
    assert_eq!(
        run_command(&f, &["send-keys", "-t", "missing", "-l", "x"]),
        1
    );
}

#[test]
fn paste_inserts_text_and_submit_adds_a_newline() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "pa", "cat"]), 0);
    assert_eq!(run_command(&f, &["paste", "-t", "pa", "pasted text"]), 0);
    let wait = ["wait", "pa", "--match", "pasted text", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    let submit = ["paste", "-t", "pa", "--submit", "submitted"];
    assert_eq!(run_command(&f, &submit), 0);
    let wait = ["wait", "pa", "--match", "submitted", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    assert!(f.logs("pa").contains("submitted\n"));
}

#[test]
fn wait_returns_the_child_exit_code() {
    let f = Fixture::start();
    assert_eq!(
        run_command(&f, &["run", "-n", "w", "sh", "-c", "exit 7"]),
        0
    );
    assert_eq!(run_command(&f, &["wait", "w", "--timeout", "5"]), 7);
}

#[test]
fn wait_for_a_match_exits_zero_once_the_output_appears() {
    let f = Fixture::start();
    let script = "echo ready; sleep 30";
    assert_eq!(run_command(&f, &["run", "-n", "m", "sh", "-c", script]), 0);
    let wait = ["wait", "m", "--match", "ready", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
}

#[test]
fn wait_times_out_with_124() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "s", "sleep", "30"]), 0);
    let wait = ["wait", "s", "--match", "never-printed", "--timeout", "1"];
    assert_eq!(run_command(&f, &wait), 124);
}

#[test]
fn wait_rejects_conflicting_or_unknown_conditions() {
    let f = Fixture::start();
    let conflicting = ["wait", "x", "--idle", "5", "--match", "a"];
    assert_eq!(run_command(&f, &conflicting), 1);
    assert_eq!(run_command(&f, &["wait", "x", "--state", "bogus"]), 1);
}

#[test]
fn logs_reads_history_in_each_format_and_rejects_unknown_nodes() {
    let f = Fixture::start();
    let script = "echo one";
    assert_eq!(run_command(&f, &["run", "-n", "l", "sh", "-c", script]), 0);
    assert_eq!(run_command(&f, &["wait", "l", "--timeout", "5"]), 0);
    assert_eq!(run_command(&f, &["logs", "l"]), 0);
    assert_eq!(run_command(&f, &["logs", "l", "--plain", "-n", "1"]), 0);
    assert_eq!(run_command(&f, &["logs", "l", "--json", "--since", "0"]), 0);
    assert_eq!(run_command(&f, &["logs", "missing"]), 1);
    assert!(f.logs("l").contains("one"));
}

#[test]
fn logs_follow_returns_once_the_process_has_exited() {
    let f = Fixture::start();
    let script = "echo first; sleep 0.2; echo second";
    assert_eq!(run_command(&f, &["run", "-n", "fo", "sh", "-c", script]), 0);
    assert_eq!(run_command(&f, &["logs", "fo", "--follow"]), 0);
    let output = f.logs("fo");
    assert!(
        output.contains("first") && output.contains("second"),
        "{output:?}"
    );
}

#[test]
fn status_reports_known_nodes_only() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "job", "true"]), 0);
    assert_eq!(run_command(&f, &["wait", "job", "--timeout", "5"]), 0);
    assert_eq!(run_command(&f, &["status", "job"]), 0);
    assert_eq!(run_command(&f, &["status", "missing"]), 1);
}

#[test]
fn kill_terminates_the_node_and_rejects_unknown_names() {
    let f = Fixture::start();
    assert_eq!(run_command(&f, &["run", "-n", "k", "sleep", "30"]), 0);
    assert_eq!(run_command(&f, &["kill", "k"]), 0);
    assert_eq!(run_command(&f, &["wait", "k", "--timeout", "5"]), 0);
    assert_eq!(f.node("k").state, NodeState::Exited);
    assert_eq!(run_command(&f, &["kill", "missing"]), 1);
}

#[test]
fn run_passes_dash_c_to_the_command() {
    let f = Fixture::start();
    let script = "echo dash-c-reached";
    assert_eq!(run_command(&f, &["run", "-n", "rc", "sh", "-c", script]), 0);
    assert_eq!(run_command(&f, &["wait", "rc", "--timeout", "5"]), 0);
    assert!(f.logs("rc").contains("dash-c-reached"));
}
