use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

use mpx::client::Client;
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::IpcListener;
use mpx::layout::{Direction, WindowTab};
use mpx::protocol::{NewWindowOpts, SpawnOpts, SplitOpts, WaitCondition};

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

/// Asserts that the daemon refused a request: `result` is an error containing `needle`.
pub fn assert_refused(result: Result<(), String>, needle: &str) {
    let err = result.expect_err("the daemon refuses the request");
    assert!(err.contains(needle), "error: {err}");
}

/// The window tabs of the session, in window order (one layout request).
pub async fn tabs(client: &mut Client) -> Vec<WindowTab> {
    client.layout(80, 24).await.unwrap().2
}

/// Each window tab as `(name, is_active)`, in window order.
pub async fn tab_states(client: &mut Client) -> Vec<(String, bool)> {
    tabs(client).await.into_iter().map(|t| (t.name, t.is_active)).collect()
}

/// A window named `label` whose only pane `pane` runs `cat`.
pub fn window(label: &str, pane: &str) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.to_string()),
        spawn: task(pane, "cat", &[]),
        focus: true,
    }
}

/// Opens windows `a`, `b` and `c` and focuses the first.
pub async fn three_windows(client: &mut Client) {
    for (label, pane) in [("a", "pa"), ("b", "pb"), ("c", "pc")] {
        client.new_window(window(label, pane)).await.unwrap();
    }
    client.select_window(0).await.unwrap();
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
        keep_zoom: false,
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

/// An absolute path to a POSIX `sh`, for a daemon's `default-shell`: `/bin/sh` on unix, and on
/// Windows the `sh.exe` that Git for Windows puts on `PATH`. Panics without one, since the tests
/// that use it run `sh` commands.
pub fn sh_path() -> String {
    if !cfg!(windows) {
        return "/bin/sh".to_string();
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("sh.exe"))
        .find(|candidate| candidate.is_file())
        .expect("a POSIX sh.exe on PATH")
        .to_string_lossy()
        .into_owned()
}
