use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

use mpx::client::Client;
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::IpcListener;
use mpx::layout::Direction;
use mpx::protocol::{SpawnOpts, SplitOpts, WaitCondition};

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
    let endpoint = serve_session(dir.path(), SESSION, state_dir, config).await;
    TestDaemon {
        endpoint,
        _dir: dir,
    }
}

/// Serves `session` from `socket_dir`, so several sessions can share one directory as they do in
/// real use. Returns its endpoint.
pub async fn serve_session(
    socket_dir: &Path,
    session: &str,
    state_dir: &Path,
    config: Config,
) -> String {
    let endpoint = mpx::ipc::endpoint_in(socket_dir, session);
    let listener = IpcListener::bind(&endpoint).expect("bind");
    let state = DaemonState::new(
        session.to_string(),
        endpoint.clone(),
        config,
        state_dir.to_path_buf(),
    )
    .await;
    tokio::spawn(accept_clients(listener, state));
    endpoint
}

async fn accept_clients(
    #[allow(unused_mut)] mut listener: IpcListener,
    state: Arc<DaemonState>,
) {
    while let Ok(stream) = listener.accept().await {
        tokio::spawn(handle_client(Arc::clone(&state), stream));
    }
}

pub async fn connect(endpoint: &str) -> Client {
    Client::connect(endpoint).await.expect("connect")
}

/// A started daemon, connected to, with `spawn` already running. The daemon is returned so the
/// caller keeps it alive.
pub async fn started_with(spawn: SpawnOpts) -> (TestDaemon, Client) {
    let (daemon, mut client) = fresh().await;
    client.spawn(spawn).await.unwrap();
    (daemon, client)
}

/// A started daemon, connected to, with nothing running.
pub async fn fresh() -> (TestDaemon, Client) {
    let daemon = start().await;
    let client = connect(&daemon.endpoint).await;
    (daemon, client)
}

/// A focused `cat` pane `name` split off `target`, side by side.
pub fn split_right(target: &str, name: &str) -> SplitOpts {
    split_with(target, name, true, None)
}

/// A `cat` pane `name` split off `target` in the horizontal direction.
pub fn split_with(target: &str, name: &str, focus: bool, percent: Option<u8>) -> SplitOpts {
    SplitOpts {
        target: Some(target.into()),
        spawn: task(name, "cat", &[]),
        direction: Direction::Horizontal,
        percent,
        focus,
    }
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
