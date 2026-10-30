use super::support::{connect, pty, start, task};
use mpx::protocol::WaitCondition;

#[tokio::test]
async fn exit_condition_reports_the_exit_code() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("exiter", "sh", &["-c", "exit 3"]))
        .await
        .unwrap();
    let outcome = client
        .wait("exiter", WaitCondition::Exit, Some(2_000))
        .await
        .unwrap();
    assert!(outcome.satisfied);
    assert_eq!(outcome.exit_code, Some(3));
}

#[tokio::test]
async fn match_returns_the_matched_output() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("matcher", "sh", &["-c", "sleep 0.1; echo token42"]))
        .await
        .unwrap();
    let outcome = client
        .wait(
            "matcher",
            WaitCondition::Match {
                pattern: "token[0-9]+".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();
    assert!(outcome.satisfied);
    assert_eq!(outcome.matched.as_deref(), Some("token42"));
    client.kill("matcher").await.unwrap();
}

#[tokio::test]
async fn match_sees_text_already_on_screen() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(pty(
            "screen-hit",
            "sh",
            &["-c", "echo already_here; sleep 5"],
        ))
        .await
        .unwrap();
    client
        .wait(
            "screen-hit",
            WaitCondition::Match {
                pattern: "already_here".into(),
            },
            Some(2_000),
        )
        .await
        .unwrap();
    client.kill("screen-hit").await.unwrap();
}

#[tokio::test]
async fn idle_condition_is_satisfied_after_quiet_period() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("quiet", "sleep", &["5"])).await.unwrap();
    let outcome = client
        .wait("quiet", WaitCondition::Idle { quiet_ms: 100 }, Some(2_000))
        .await
        .unwrap();
    assert!(outcome.satisfied);
    client.kill("quiet").await.unwrap();
}

#[tokio::test]
async fn timeout_reports_unsatisfied() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("never", "sleep", &["5"])).await.unwrap();
    let outcome = client
        .wait(
            "never",
            WaitCondition::Match {
                pattern: "never_printed".into(),
            },
            Some(100),
        )
        .await
        .unwrap();
    assert!(!outcome.satisfied);
    assert_eq!(outcome.matched, None);
    client.kill("never").await.unwrap();
}

#[tokio::test]
async fn invalid_regex_is_an_error() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("bad-re", "sleep", &["5"])).await.unwrap();
    let result = client
        .wait(
            "bad-re",
            WaitCondition::Match {
                pattern: "(".into(),
            },
            Some(100),
        )
        .await;
    assert!(result.is_err());
    client.kill("bad-re").await.unwrap();
}

