use std::time::{Duration, Instant};

use super::support::{connect, start, task};
use mpx::protocol::WaitCondition;

#[tokio::test]
async fn exit_wait_resolves_as_soon_as_the_process_ends() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("quick", "sh", &["-c", "exit 0"]))
        .await
        .unwrap();

    let started = Instant::now();
    let outcome = client
        .wait("quick", WaitCondition::Exit, Some(2_000))
        .await
        .unwrap();
    assert!(outcome.satisfied);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn match_wait_resolves_on_the_output_that_matches() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task(
            "late",
            "sh",
            &["-c", "sleep 0.05; echo ready_now; sleep 5"],
        ))
        .await
        .unwrap();

    let started = Instant::now();
    let outcome = client
        .wait(
            "late",
            WaitCondition::Match {
                pattern: "ready_now".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();
    assert!(outcome.satisfied);
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
    client.kill("late").await.unwrap();
}

#[tokio::test]
async fn idle_wait_resolves_one_quiet_period_after_the_last_output() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("chatty", "sh", &["-c", "echo x; sleep 5"]))
        .await
        .unwrap();

    let started = Instant::now();
    let outcome = client
        .wait("chatty", WaitCondition::Idle { quiet_ms: 100 }, Some(2_000))
        .await
        .unwrap();
    let took = started.elapsed();
    assert!(outcome.satisfied);
    assert!(took >= Duration::from_millis(90), "{took:?}");
    assert!(took < Duration::from_millis(500), "{took:?}");
    client.kill("chatty").await.unwrap();
}

#[tokio::test]
async fn timeout_returns_unsatisfied_at_the_deadline() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("slow", "sleep", &["5"])).await.unwrap();

    let started = Instant::now();
    let outcome = client
        .wait("slow", WaitCondition::Exit, Some(150))
        .await
        .unwrap();
    let took = started.elapsed();
    assert!(!outcome.satisfied);
    assert!(took >= Duration::from_millis(140), "{took:?}");
    assert!(took < Duration::from_millis(500), "{took:?}");
    client.kill("slow").await.unwrap();
}
