use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::mpsc;

use mpx::node::SpawnSpec;
use mpx::node::pacing::{PACE_HIGH, Pacing};
use mpx::node::pane::{self, PaneCtx, PaneHandle, PaneNotice, TermView};
use mpx::protocol::{NodeState, WaitCondition};

fn spawn_task(name: &str, command: &str) -> (PaneHandle, mpsc::UnboundedReceiver<PaneNotice>) {
    spawn_paced(name, command, Arc::default())
}

fn spawn_paced(
    name: &str,
    command: &str,
    pacing: Arc<Pacing>,
) -> (PaneHandle, mpsc::UnboundedReceiver<PaneNotice>) {
    let spec = SpawnSpec {
        name: name.to_string(),
        id: 0,
        command: command.to_string(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        env_remove: Vec::new(),
        term: "xterm-256color".to_string(),
        scrollback: 100_000,
        restored: Vec::new(),
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = PaneCtx {
        notify: Arc::new(move |notice| {
            let _ = tx.send(notice);
        }),
        relay: Arc::new(AtomicBool::new(true)),
        pacing,
    };
    let handle = pane::spawn(spec, false, ctx).expect("spawn");
    (handle, rx)
}

/// Next exit notice, skipping output notices (bounded).
async fn next_exit(notices: &mut mpsc::UnboundedReceiver<PaneNotice>) -> Option<i32> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match notices.recv().await {
                Some(PaneNotice::Exited { code, .. }) => return code,
                Some(PaneNotice::Event(_)) => {}
                None => panic!("pane ended without an exit notice"),
            }
        }
    })
    .await
    .expect("exit reported to the session")
}

#[tokio::test]
async fn a_task_pane_holds_its_output_while_a_terminal_is_behind() {
    let pacing = Arc::new(Pacing::default());
    let terminal = pacing.track(mpx::node::now_ms());
    for _ in 0..=PACE_HIGH {
        terminal.queued(mpx::node::now_ms());
    }
    let (task, _notices) = spawn_paced("paced", "cat", Arc::clone(&pacing));
    task.input(Box::new(|_: &TermView| b"ping\n".to_vec()))
        .await
        .unwrap();
    let ping = || WaitCondition::Match {
        pattern: "ping".into(),
    };
    let held = task
        .wait(ping(), Some(Duration::from_millis(300)))
        .await
        .unwrap();
    assert!(
        !held.satisfied,
        "the output waits while the terminal is behind"
    );

    terminal.sent(mpx::node::now_ms());
    let seen = task
        .wait(ping(), Some(Duration::from_secs(1)))
        .await
        .unwrap();
    assert!(seen.satisfied, "and comes once it catches up");
    task.request_kill().await.expect("killed");
}

#[tokio::test]
async fn task_pane_pipes_stdin_and_reports_kill_as_exit() {
    let (task, mut notices) = spawn_task("direct", "cat");
    assert_eq!(task.info(false).await.unwrap().state, NodeState::Running);

    task.input(Box::new(|_: &TermView| b"ping\n".to_vec()))
        .await
        .unwrap();
    let seen = task
        .wait(
            WaitCondition::Match {
                pattern: "ping".into(),
            },
            Some(Duration::from_secs(2)),
        )
        .await
        .unwrap();
    assert!(seen.satisfied, "stdin was echoed");

    let exited = task.request_kill();
    tokio::time::timeout(Duration::from_secs(2), exited)
        .await
        .expect("kill ends the process")
        .expect("exit acknowledged");
    assert_eq!(task.info(false).await.unwrap().state, NodeState::Exited);
    next_exit(&mut notices).await;
}
