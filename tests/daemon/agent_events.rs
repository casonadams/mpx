use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::support::{connect, start_with, task};
use mpx::config::{AgentConfig, Config, HarnessConfig};
use mpx::protocol::{AgentState, StreamEvent};
use tokio::sync::mpsc::UnboundedReceiver;

/// Consumes agent states until `want` arrives (bounded), recording everything seen.
async fn until(
    rx: &mut UnboundedReceiver<Option<AgentState>>,
    seen: &mut Vec<AgentState>,
    want: AgentState,
) {
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(state) = rx.recv().await {
            if let Some(state) = state {
                seen.push(state);
                if state == want {
                    return true;
                }
            }
        }
        false
    })
    .await;
    assert!(
        matches!(found, Ok(true)),
        "never reached {want:?}; seen {seen:?}"
    );
}

/// An agent without hooks, named so the harness detection matches the script's own name. It rings
/// the bell only when told to, once the test has seen it idle: an alert before the agent is
/// recognized (the daemon probes processes every 250 ms) would not count.
fn fake_agent(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("fakeagent");
    let body = "#!/bin/sh\nsleep 0.1\necho working\nread ring\nprintf '\\a'\nread line\necho \"got $line\"\nexec sleep 5\n";
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[tokio::test]
async fn unhooked_agent_transitions_stream_to_session_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_agent(dir.path());

    let config = Config {
        harness: vec![HarnessConfig {
            name: "fakeagent".to_string(),
            detect: vec!["fakeagent".to_string()],
            ..HarnessConfig::default()
        }],
        agent: AgentConfig {
            idle_ms: 150,
            ..AgentConfig::default()
        },
        ..Config::default()
    };
    let daemon = start_with(config).await;
    let mut client = connect(&daemon.endpoint).await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Option<AgentState>>();
    let endpoint = daemon.endpoint.clone();
    tokio::spawn(async move {
        let mut sub = connect(&endpoint).await;
        let _ = sub
            .stream_logs(None, None, move |event| {
                if let StreamEvent::Agent { name, agent } = event
                    && name == "fakeagent"
                {
                    let _ = tx.send(agent.map(|a| a.state));
                }
                true
            })
            .await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    client
        .spawn(task("fakeagent", script.to_str().unwrap(), &[]))
        .await
        .unwrap();

    let mut seen = Vec::new();
    until(&mut rx, &mut seen, AgentState::Working).await;
    until(&mut rx, &mut seen, AgentState::Idle).await;
    client.send_input("fakeagent", "ring\n").await.unwrap();
    until(&mut rx, &mut seen, AgentState::NeedsInput).await;

    client.send_input("fakeagent", "go\n").await.unwrap();
    until(&mut rx, &mut seen, AgentState::Working).await;

    client.kill("fakeagent").await.unwrap();
}
