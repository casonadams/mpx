use super::support::{connect, pty, start};
use mpx::client::AgentReportArgs;
use mpx::protocol::{AgentState, WaitCondition};

#[tokio::test]
async fn bell_while_an_agent_runs_needs_input_until_typed_into() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty(
            "bell-agent",
            "sh",
            &["-c", "echo READY; sleep 0.5; printf '\\007'; cat"],
        ))
        .await
        .unwrap();
    client
        .agent_report(
            "bell-agent",
            AgentReportArgs {
                harness: Some("claude".into()),
                state: Some(AgentState::Working),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let prompted = client
        .wait(
            "bell-agent",
            WaitCondition::State {
                state: AgentState::NeedsInput,
            },
            Some(2_000),
        )
        .await
        .unwrap();
    assert!(prompted.satisfied, "bell should raise NeedsInput");

    client.send_input("bell-agent", "answer\n").await.unwrap();
    let resumed = client
        .wait(
            "bell-agent",
            WaitCondition::State {
                state: AgentState::Working,
            },
            Some(2_000),
        )
        .await
        .unwrap();
    assert!(resumed.satisfied, "input should clear NeedsInput");
    client.kill("bell-agent").await.unwrap();
}

#[tokio::test]
async fn clear_report_removes_agent_info() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(super::support::task("cleared", "sleep", &["5"]))
        .await
        .unwrap();
    client
        .agent_report(
            "cleared",
            AgentReportArgs {
                harness: Some("omp".into()),
                state: Some(AgentState::Idle),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(client.status("cleared").await.unwrap().agent.is_some());

    client
        .agent_report(
            "cleared",
            AgentReportArgs {
                clear: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(client.status("cleared").await.unwrap().agent.is_none());
    client.kill("cleared").await.unwrap();
}
