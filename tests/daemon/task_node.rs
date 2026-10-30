use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::mpsc;

use mpx::node::SpawnSpec;
use mpx::node::pane::{self, PaneCtx, PaneHandle, PaneNotice, TermView};
use mpx::protocol::{NodeState, WaitCondition};

fn spawn_task(name: &str, command: &str) -> (PaneHandle, mpsc::UnboundedReceiver<PaneNotice>) {
    let spec = SpawnSpec {
        name: name.to_string(),
        id: 0,
        command: command.to_string(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = PaneCtx {
        notify: Arc::new(move |notice| {
            let _ = tx.send(notice);
        }),
        relay: Arc::new(AtomicBool::new(true)),
        pacing: Arc::default(),
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
