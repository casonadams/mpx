use std::ffi::OsString;
#[cfg(unix)]
use std::io::ErrorKind;
use std::time::Duration;

use super::panes;
use super::render;
use super::resolve::{
    Resolved, load_config, nested_attach_refused, resolve, resolve_idempotency_key,
};
use super::{Cli, Commands};
use crate::client::{Client, Handshake};
use crate::daemon::run_daemon;
use crate::ipc;
use crate::store;
use crate::tui::app::AppOptions;
use crate::tui::status::{clock_hms, local_utc_offset};
use crate::tui::{StartView, run_tui};

const SPAWN_RETRIES: u32 = 20;
const SHUTDOWN_POLLS: u32 = 20;

pub async fn run_cli(cli: Cli) -> Result<i32, String> {
    let mut target = resolve(
        cli.session.as_deref(),
        cli.endpoint.as_deref(),
        env_var("MPX_SESSION").as_deref(),
        env_var("MPX_ENDPOINT").as_deref(),
        &ipc::socket_dir(),
    );
    target.idempotency_key = resolve_idempotency_key(cli.idempotency_key.as_deref());
    target.config = load_config(cli.config.as_deref())?;
    target.config_path = cli.config;
    if let Some(dir) = cli.state_dir {
        target.state_dir = dir;
    }
    let command = cli.command.unwrap_or(Commands::Attach {
        target: None,
        force: false,
    });
    dispatch(command, target).await
}

fn env_var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

async fn dispatch(command: Commands, r: Resolved) -> Result<i32, String> {
    match command {
        Commands::Daemon => {
            run_daemon(&r.endpoint, &r.session, &r.state_dir, r.config)
                .await
                .map_err(|e| e.to_string())?;
            Ok(0)
        }
        Commands::KillServer => kill_server(&r).await,
        Commands::Attach { target, force } => attach(&r, target, force).await,
        Commands::ListSessions { json } => list_sessions(&r, json).await,
        Commands::ListPanes { json } => list_panes(&r, json).await,
        Commands::HasSession { target } => has_session(&r, target.as_deref()).await,
        Commands::RenameSession { target, name } => {
            rename_session(&r, target.as_deref(), &name).await
        }
        Commands::Restore => restore(&r).await,
        other => panes::run(other, &r).await,
    }
}

/// Connects to the session, starting its daemon when it is not running.
pub async fn ensure_client(r: &Resolved) -> Result<Client, String> {
    match probe(&r.endpoint).await {
        Probe::Ready(client) => return Ok(client),
        Probe::Incompatible(pid) => return Err(incompatible(&r.endpoint, pid)),
        Probe::Down => {}
    }
    clear_stale_socket(&r.endpoint).await;
    spawn_daemon(r)?;
    for _ in 0..SPAWN_RETRIES {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Probe::Ready(client) = probe(&r.endpoint).await {
            return Ok(client);
        }
    }
    Err(format!(
        "Could not connect to mpx daemon at '{}'",
        r.endpoint
    ))
}

/// Connects to a running daemon without starting one.
pub async fn connect_running(endpoint: &str) -> Result<Client, String> {
    match probe(endpoint).await {
        Probe::Ready(client) => Ok(client),
        Probe::Incompatible(pid) => Err(incompatible(endpoint, pid)),
        Probe::Down => Err(format!("no server running on {endpoint}")),
    }
}

/// What answers on a session socket.
enum Probe {
    Ready(Client),
    /// A daemon from another mpx version, with its pid when known.
    Incompatible(Option<u32>),
    Down,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

async fn probe(endpoint: &str) -> Probe {
    let probe_fut = async {
        let Ok(mut client) = Client::connect(endpoint).await else {
            return Probe::Down;
        };
        match client.handshake().await {
            Handshake::Compatible => Probe::Ready(client),
            Handshake::Incompatible { pid } => Probe::Incompatible(pid),
            Handshake::Gone => Probe::Down,
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT, probe_fut)
        .await
        .unwrap_or(Probe::Down)
}

fn incompatible(endpoint: &str, pid: Option<u32>) -> String {
    let who = pid.map_or_else(String::new, |pid| format!(" (pid {pid})"));
    format!(
        "the mpx daemon on {endpoint}{who} is from an incompatible mpx version; \
         stop it with `mpx kill-server` for this session (its panes close), then run mpx again"
    )
}

/// Removes a socket left behind by a crashed daemon; any other connect error leaves the file alone.
#[cfg(unix)]
async fn clear_stale_socket(endpoint: &str) {
    let stale = matches!(
        ipc::connect_stream(endpoint).await,
        Err(e) if matches!(e.kind(), ErrorKind::ConnectionRefused | ErrorKind::NotFound)
    );
    if stale {
        let _ = std::fs::remove_file(endpoint);
    }
}

#[cfg(not(unix))]
async fn clear_stale_socket(_endpoint: &str) {}

/// Arguments that make an auto-spawned daemon serve this session with the same endpoint, config and
/// state directory as this command.
pub fn daemon_args(r: &Resolved) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--session".into(),
        r.session.as_str().into(),
        "--endpoint".into(),
        r.endpoint.as_str().into(),
        "--state-dir".into(),
        r.state_dir.as_os_str().to_owned(),
    ];
    if let Some(path) = &r.config_path {
        args.push("-f".into());
        args.push(path.as_os_str().to_owned());
    }
    args.push("daemon".into());
    args
}

fn spawn_daemon(r: &Resolved) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(daemon_args(r))
        .env_remove("MPX_PANE")
        .env_remove("MPX_ENDPOINT")
        .env_remove("TMUX")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    // Its own session, without a controlling terminal: closing the terminal that started it sends
    // SIGHUP to that terminal's processes, which would otherwise take the daemon and every pane.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches nothing shared with the parent.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to auto-spawn mpx daemon: {e}"))
}

async fn kill_server(r: &Resolved) -> Result<i32, String> {
    match probe(&r.endpoint).await {
        Probe::Ready(mut client) => {
            let _ = client.shutdown().await;
        }
        Probe::Incompatible(Some(pid)) => terminate(pid)?,
        Probe::Incompatible(None) => return Err(incompatible(&r.endpoint, None)),
        Probe::Down => return Err(format!("no server running on {}", r.endpoint)),
    }
    for _ in 0..SHUTDOWN_POLLS {
        if matches!(probe(&r.endpoint).await, Probe::Down) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(0)
}

/// Stops a daemon that cannot be asked to shut down, such as one from another mpx version.
#[cfg(unix)]
fn terminate(pid: u32) -> Result<(), String> {
    if pid == std::process::id() {
        return Err("refusing to stop this process: it serves the session itself".to_string());
    }
    let pid = libc::pid_t::try_from(pid).map_err(|e| e.to_string())?;
    // SAFETY: kill(2) only signals; the pid is the peer of the session socket we just talked to.
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(format!(
            "could not stop daemon pid {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn terminate(pid: u32) -> Result<(), String> {
    Err(format!(
        "the daemon (pid {pid}) is from an incompatible mpx version; stop that process manually"
    ))
}

/// Makes the next request of `client` idempotent under `key`, when a key was given.
pub fn apply_key(client: &mut Client, key: Option<&str>) {
    if let Some(key) = key {
        client.idempotency_key(key);
    }
}

/// What `restore_session` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The daemon already restored its snapshot at startup, at this Unix time in milliseconds.
    AlreadyRestored { restored_ms: u64 },
    /// The snapshot was replayed; the session now has these windows and panes.
    Restored { windows: usize, panes: usize },
}

/// Recreates the session from its snapshot, unless the daemon already did so when it started.
pub async fn restore_session(client: &mut Client) -> Result<RestoreOutcome, String> {
    let info = client.info().await?;
    if let Some(restored_ms) = info.restored_ms {
        return Ok(RestoreOutcome::AlreadyRestored { restored_ms });
    }
    client
        .restore()
        .await
        .map_err(|e| format!("Restore error: {e}"))?;
    let info = client.info().await?;
    Ok(RestoreOutcome::Restored {
        windows: info.windows,
        panes: info.panes,
    })
}

async fn restore(r: &Resolved) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    match restore_session(&mut client).await? {
        RestoreOutcome::AlreadyRestored { restored_ms } => println!(
            "Session '{}' already restored from its snapshot at {}",
            r.session,
            clock_hms(restored_ms, local_utc_offset(restored_ms))
        ),
        RestoreOutcome::Restored { windows, panes } => println!(
            "Session '{}' restored: {windows} windows, {panes} panes",
            r.session
        ),
    }
    Ok(0)
}

fn target_resolved(current: &Resolved, target: &str) -> Resolved {
    let endpoint = if target == current.session {
        current.endpoint.clone()
    } else {
        ipc::endpoint_in(&current.socket_dir, target)
    };
    let state_dir = if target == current.session {
        current.state_dir.clone()
    } else {
        store::session_dir(target)
    };
    Resolved {
        session: target.to_string(),
        endpoint,
        socket_dir: current.socket_dir.clone(),
        idempotency_key: None,
        config_path: current.config_path.clone(),
        config: current.config.clone(),
        state_dir,
    }
}

async fn attach(r: &Resolved, target: Option<String>, force: bool) -> Result<i32, String> {
    if nested_attach_refused(env_var("MPX_ENDPOINT").is_some(), force) {
        return Err(
            "sessions should be nested with care, unset $MPX_ENDPOINT to force".to_string(),
        );
    }
    let target = match target {
        Some(t) => target_resolved(r, &t),
        None => r.clone(),
    };
    ensure_client(&target).await?;
    run_tui(
        &target.endpoint,
        &target.session,
        StartView::Panes,
        tui_options(&target),
    )
    .await?;
    Ok(0)
}

pub async fn has_session(r: &Resolved, target: Option<&str>) -> Result<i32, String> {
    let target = match target {
        Some(t) => target_resolved(r, t.rsplit(':').next().unwrap_or(t)),
        None => r.clone(),
    };
    Ok(match probe(&target.endpoint).await {
        Probe::Down => 1,
        Probe::Ready(_) | Probe::Incompatible(_) => 0,
    })
}

async fn list_sessions(r: &Resolved, json: bool) -> Result<i32, String> {
    let infos = ipc::session_infos(&r.socket_dir).await;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&infos).map_err(|e| e.to_string())?
        );
    } else {
        for info in &infos {
            println!(
                "{}: {} windows, {} panes ({})",
                info.name, info.windows, info.panes, info.endpoint
            );
        }
    }
    Ok(0)
}

async fn list_panes(r: &Resolved, json: bool) -> Result<i32, String> {
    let mut client = connect_running(&r.endpoint).await?;
    let nodes = client
        .list()
        .await
        .map_err(|e| format!("List error: {e}"))?;
    print!("{}", render::panes(&nodes, json)?);
    Ok(0)
}
async fn rename_session(r: &Resolved, target: Option<&str>, new_name: &str) -> Result<i32, String> {
    let resolved = match target {
        Some(t) => target_resolved(r, t.rsplit(':').next().unwrap_or(t)),
        None => r.clone(),
    };
    let mut client = ensure_client(&resolved).await?;
    client
        .rename_session(new_name)
        .await
        .map_err(|e| format!("Rename session error: {e}"))?;
    Ok(0)
}
/// What the attached TUI needs from this invocation: its config, socket directory and the real terminal size.
pub fn tui_options(r: &Resolved) -> AppOptions {
    AppOptions {
        config: r.config.clone(),
        socket_dir: r.socket_dir.clone(),
        size: None,
    }
}
