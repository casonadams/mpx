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

pub async fn start() -> TestDaemon {
    start_with(Config::default()).await
}

pub async fn start_with(config: Config) -> TestDaemon {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint = dir.path().join("mpx.sock").to_string_lossy().into_owned();
    let listener = IpcListener::bind(&endpoint).expect("bind");
    let state = DaemonState::new(SESSION.to_string(), endpoint.clone(), config);

    let accept_state = Arc::clone(&state);
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            tokio::spawn(handle_client(Arc::clone(&accept_state), stream));
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

/// Polls logs until `needle` appears (bounded); returns the last logs seen.
pub async fn logs_until(client: &mut Client, name: &str, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let (data, _) = client.logs(name, None, false, None).await.expect("logs");
        if data.contains(needle) || Instant::now() >= deadline {
            return data;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
