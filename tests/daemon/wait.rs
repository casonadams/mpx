use mpx::client::Client;
use mpx::protocol::{SpawnOpts, WaitCondition, WaitOutcome};

use super::support::{fresh, pty, task};

/// Starts `spawn` as a pane and waits on it for `condition`, for at most `timeout_ms`.
async fn spawn_and_wait(
    client: &mut Client,
    spawn: SpawnOpts,
    condition: WaitCondition,
    timeout_ms: u64,
) -> WaitOutcome {
    let name = spawn.name.clone();
    client.spawn(spawn).await.unwrap();
    client
        .wait(&name, condition, Some(timeout_ms))
        .await
        .unwrap()
}

fn matching(pattern: &str) -> WaitCondition {
    WaitCondition::Match {
        pattern: pattern.into(),
    }
}

/// Each condition reports its own outcome: an exit code, the matched text, a match already on
/// screen, a quiet period, and a timeout that stays unsatisfied.
#[tokio::test]
async fn wait_outcomes_follow_the_condition() {
    let (_daemon, mut client) = fresh().await;

    let spawn = task("exiter", "sh", &["-c", "exit 3"]);
    let outcome = spawn_and_wait(&mut client, spawn, WaitCondition::Exit, 2_000).await;
    assert!(outcome.satisfied);
    assert_eq!(outcome.exit_code, Some(3));

    let spawn = task("matcher", "sh", &["-c", "sleep 0.1; echo token42"]);
    let outcome = spawn_and_wait(&mut client, spawn, matching("token[0-9]+"), 2_000).await;
    assert!(outcome.satisfied);
    assert_eq!(outcome.matched.as_deref(), Some("token42"));
    client.kill("matcher").await.unwrap();

    let spawn = pty("screen-hit", "sh", &["-c", "echo already_here; sleep 5"]);
    let outcome = spawn_and_wait(&mut client, spawn, matching("already_here"), 2_000).await;
    assert!(outcome.satisfied);
    client.kill("screen-hit").await.unwrap();

    let spawn = task("quiet", "sleep", &["5"]);
    let idle = WaitCondition::Idle { quiet_ms: 100 };
    let outcome = spawn_and_wait(&mut client, spawn, idle, 2_000).await;
    assert!(outcome.satisfied);
    client.kill("quiet").await.unwrap();

    let spawn = task("never", "sleep", &["5"]);
    let outcome = spawn_and_wait(&mut client, spawn, matching("never_printed"), 100).await;
    assert!(!outcome.satisfied);
    assert_eq!(outcome.matched, None);
    client.kill("never").await.unwrap();
}

#[tokio::test]
async fn invalid_regex_is_an_error() {
    let (_daemon, mut client) = fresh().await;
    client.spawn(task("bad-re", "sleep", &["5"])).await.unwrap();
    let result = client.wait("bad-re", matching("("), Some(100)).await;
    assert!(result.is_err());
    client.kill("bad-re").await.unwrap();
}
