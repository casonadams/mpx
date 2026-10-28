use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

use mpx::client::Client;
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::IpcListener;
use mpx::protocol::{SpawnOpts, WaitCondition};

pub const SESSION: &str = "test";

pub struct TestDaemon {
    pub endpoint: String,
    _dir: TempDir,
}

/// A daemon with a fresh state directory.
pub async fn start() -> TestDaemon {
    start_with(Config::default()).await
}

pub async fn start_with(config: Config) -> TestDaemon {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    serve(dir, &state_dir, config).await
}

/// A daemon whose durable state lives in `state_dir`, so it can be restarted on the same state.
pub async fn start_on(state_dir: &Path, config: Config) -> TestDaemon {
    let dir = tempfile::tempdir().expect("tempdir");
    serve(dir, state_dir, config).await
}

async fn serve(dir: TempDir, state_dir: &Path, config: Config) -> TestDaemon {
    let endpoint = mpx::ipc::endpoint_in(dir.path(), SESSION);
    #[allow(unused_mut)]
    let mut listener = IpcListener::bind(&endpoint).expect("bind");
    let state = DaemonState::new(
        SESSION.to_string(),
        endpoint.clone(),
        config,
        state_dir.to_path_buf(),
    )
    .await;

    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            tokio::spawn(handle_client(Arc::clone(&state), stream));
        }
    });

    TestDaemon {
        endpoint,
        _dir: dir,
    }
}

pub async fn connect(endpoint: &str) -> Client {
    Client::connect(endpoint).await.expect("connect")
}

pub fn task(name: &str, command: &str, args: &[&str]) -> SpawnOpts {
    SpawnOpts {
        name: name.to_string(),
        command: command.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        ..SpawnOpts::default()
    }
}

pub fn pty(name: &str, command: &str, args: &[&str]) -> SpawnOpts {
    SpawnOpts {
        pty: true,
        ..task(name, command, args)
    }
}

/// Waits for the process to exit (bounded) and returns its exit code.
pub async fn exit_code(client: &mut Client, name: &str) -> Option<i32> {
    let outcome = client
        .wait(name, WaitCondition::Exit, Some(5_000))
        .await
        .expect("wait");
    assert!(outcome.satisfied, "{name} did not exit in time");
    outcome.exit_code
}

/// Returns the logs once `needle` appears in them; waits on the daemon (bounded) otherwise.
pub async fn logs_until(client: &mut Client, name: &str, needle: &str) -> String {
    let (data, _) = client.logs(name, None, false, None).await.expect("logs");
    if data.contains(needle) {
        return data;
    }
    let outcome = client
        .wait(
            name,
            WaitCondition::Match {
                pattern: regex::escape(needle),
            },
            Some(2_000),
        )
        .await
        .expect("wait");
    assert!(outcome.satisfied, "'{needle}' never appeared in {name}");
    let (data, _) = client.logs(name, None, false, None).await.expect("logs");
    data
}

/// Polls `check` until it yields a value, failing after `timeout`.
pub async fn eventually<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "condition not met in time");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
