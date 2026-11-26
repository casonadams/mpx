use std::sync::Arc;
use std::time::{Duration, Instant};

use mpx::node::pacing::{Inflight, Pacing};
use mpx::node::pane::PaneMsg;
use mpx::node::pty::PtyNode;
use mpx::node::{Process, SpawnSpec};
use tokio::sync::mpsc::{self, UnboundedReceiver};

fn spawn_sh(script: &str) -> (PtyNode, UnboundedReceiver<PaneMsg>) {
    spawn_sh_sized(script, (80, 24))
}

fn spawn_sh_sized(script: &str, size: (u16, u16)) -> (PtyNode, UnboundedReceiver<PaneMsg>) {
    let spec = SpawnSpec {
        name: "stubborn".to_string(),
        id: 1,
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        cwd: None,
        env: Vec::new(),
        env_remove: Vec::new(),
        term: "xterm-256color".to_string(),
        scrollback: 1000,
        restored: Vec::new(),
        size,
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let node = PtyNode::spawn(
        &spec,
        tx,
        Arc::new(Pacing::default()),
        Arc::new(Inflight::default()),
    )
    .expect("spawn sh in a PTY");
    (node, rx)
}

/// Reads messages until the pane prints `marker`, within `within`.
fn wait_for_output(rx: &mut UnboundedReceiver<PaneMsg>, marker: &[u8], within: Duration) -> bool {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < within {
        match rx.try_recv() {
            Ok(PaneMsg::Output(bytes)) => {
                seen.extend_from_slice(&bytes);
                if seen.windows(marker.len()).any(|w| w == marker) {
                    return true;
                }
            }
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    false
}

/// Waits for the pane's exit message, within `within`.
fn wait_for_exit(rx: &mut UnboundedReceiver<PaneMsg>, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        match rx.try_recv() {
            Ok(PaneMsg::Exited(_)) => return true,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    false
}

#[test]
fn a_program_starts_on_a_terminal_of_the_pane_size() {
    let (_node, mut rx) = spawn_sh_sized("echo size=$(stty size)", (57, 13));
    assert!(wait_for_output(
        &mut rx,
        b"size=13 57",
        Duration::from_secs(1)
    ));
}

#[test]
fn kill_ends_a_pane_whose_program_ignores_hangup() {
    let (node, mut rx) = spawn_sh("trap '' HUP; echo ready; while :; do sleep 0.1; done");
    assert!(wait_for_output(&mut rx, b"ready", Duration::from_secs(1)));
    node.kill().expect("kill");
    assert!(
        wait_for_exit(&mut rx, Duration::from_millis(1900)),
        "the pane exits after its kill grace period"
    );
}
