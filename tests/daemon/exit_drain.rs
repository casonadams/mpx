use super::support::{connect, pty, start_with, task};
use mpx::config::Config;
use mpx::protocol::{SpawnOpts, WaitCondition};

/// Every short-lived child's output must be present when its `Exit` is observed, for PTY and task
/// children spawned concurrently.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn output_of_fast_children_is_present_when_exit_is_observed() {
    let daemon = start_with(Config {
        history_limit: 1_000,
        ..Config::default()
    })
    .await;
    let mut runs = Vec::new();
    for i in 0..20 {
        let ep = daemon.endpoint.clone();
        runs.push(tokio::spawn(run_child(
            ep,
            format!("fast-pty-{i}"),
            format!("tok_{i}_pty"),
            pty_opts(&format!("fast-pty-{i}"), &format!("tok_{i}_pty")),
        )));
        let ep = daemon.endpoint.clone();
        runs.push(tokio::spawn(run_child(
            ep,
            format!("fast-task-{i}"),
            format!("tok_{i}_task"),
            task_opts(&format!("fast-task-{i}"), &format!("tok_{i}_task")),
        )));
    }
    for run in runs {
        run.await.unwrap();
    }
}

fn pty_opts(name: &str, token: &str) -> SpawnOpts {
    pty(name, "printf", &["%s\\n", token])
}

fn task_opts(name: &str, token: &str) -> SpawnOpts {
    task(name, "printf", &["%s\\n", token])
}

async fn run_child(endpoint: String, name: String, token: String, opts: SpawnOpts) {
    let mut client = connect(&endpoint).await;
    client.spawn(opts).await.unwrap();
    let outcome = client
        .wait(&name, WaitCondition::Exit, Some(5_000))
        .await
        .unwrap();
    assert!(outcome.satisfied, "{name} exited");
    let (logs, _) = client.logs(&name, None, false, None).await.unwrap();
    assert!(logs.contains(&token), "{name} lost its output: {logs:?}");
}
