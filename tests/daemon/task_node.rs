use std::time::Duration;
use tokio::sync::broadcast;

use mpx::node::task::TaskNode;
use mpx::node::{Node, SpawnSpec};

fn spec(name: &str, command: &str) -> SpawnSpec {
    SpawnSpec {
        name: name.to_string(),
        id: 0,
        command: command.to_string(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
    }
}

#[tokio::test]
async fn task_node_pipes_stdin_and_reports_kill_as_exit() {
    let (event_tx, _) = broadcast::channel(16);
    let task = TaskNode::spawn(spec("direct", "cat"), event_tx).unwrap();
    assert!(task.is_running());

    task.send_input(b"ping\n").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !task.read_buffer(None).contains("ping") && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(task.read_buffer(None).contains("ping"));

    task.kill().unwrap();
    let code = tokio::time::timeout(Duration::from_secs(2), task.wait_exit())
        .await
        .expect("kill ends the process");
    let _ = code;
    assert!(!task.is_running());
}
