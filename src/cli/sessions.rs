use std::io::ErrorKind;
use std::time::Duration;

use super::agents;
use super::panes;
use super::render;
use super::resolve::{Resolved, nested_attach_refused, resolve, resolve_idempotency_key};
use super::{Cli, Commands, PipeCommands};
use crate::client::{Client, PipeReadArgs};
use crate::daemon::run_daemon;
use crate::ipc;
use crate::mcp::McpServer;
use crate::tui::status::{clock_hms, local_utc_offset};
use crate::tui::{StartView, run_tui};

const SPAWN_RETRIES: u32 = 20;
const SHUTDOWN_POLLS: u32 = 20;

pub async fn run(cli: Cli) -> Result<i32, String> {
    let mut target = resolve(
        cli.session.as_deref(),
        cli.endpoint.as_deref(),
        env_var("MPX_SESSION").as_deref(),
        env_var("MPX_ENDPOINT").as_deref(),
    );
    target.idempotency_key = resolve_idempotency_key(cli.idempotency_key.as_deref());
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
            run_daemon(&r.endpoint, &r.session)
                .await
                .map_err(|e| e.to_string())?;
            Ok(0)
        }
        Commands::KillServer => kill_server(&r).await,
        Commands::Attach { target, force } => attach(&r, target, force).await,
        Commands::ListSessions { json } => list_sessions(json).await,
        Commands::ListPanes { json } => list_panes(&r, json).await,
        Commands::HasSession { target } => has_session(&r, target.as_deref()).await,
        Commands::Pipe { command } => pipe(&r, command).await,
        Commands::Restore => restore(&r).await,
        Commands::Mcp => {
            ensure_client(&r).await?;
            McpServer::new(r.endpoint, r.session)
                .run_stdio()
                .await
                .map_err(|e| format!("MCP error: {e}"))?;
            Ok(0)
        }
        Commands::Agents { json, command } => agents::agents(&r, json, command).await,
        Commands::Hook { harness, event } => {
            agents::hook(&harness, event.as_deref(), r.idempotency_key.as_deref()).await
        }
        other => panes::run(other, &r).await,
    }
}

/// Connects to the session, starting its daemon when it is not running.
pub async fn ensure_client(r: &Resolved) -> Result<Client, String> {
    if let Some(client) = probe(&r.endpoint).await {
        return Ok(client);
    }
    clear_stale_socket(&r.endpoint).await;
    spawn_daemon(r)?;
    for _ in 0..SPAWN_RETRIES {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(client) = probe(&r.endpoint).await {
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
    probe(endpoint)
        .await
        .ok_or_else(|| format!("no server running on {endpoint}"))
}

async fn probe(endpoint: &str) -> Option<Client> {
    let mut client = Client::connect(endpoint).await.ok()?;
    client.ping().await.ok()?;
    Some(client)
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

fn spawn_daemon(r: &Resolved) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg("--session")
        .arg(&r.session)
        .arg("--endpoint")
        .arg(&r.endpoint)
        .arg("daemon")
        .env_remove("MPX_PANE")
        .env_remove("MPX_ENDPOINT")
        .env_remove("TMUX")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to auto-spawn mpx daemon: {e}"))
}

async fn kill_server(r: &Resolved) -> Result<i32, String> {
    let mut client = connect_running(&r.endpoint).await?;
    let _ = client.shutdown().await;
    for _ in 0..SHUTDOWN_POLLS {
        if probe(&r.endpoint).await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(0)
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
        ipc::default_endpoint(target)
    };
    Resolved {
        session: target.to_string(),
        endpoint,
        idempotency_key: None,
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
    let _ = run_tui(&target.endpoint, &target.session, StartView::Panes).await;
    Ok(0)
}

pub async fn has_session(r: &Resolved, target: Option<&str>) -> Result<i32, String> {
    let target = match target {
        Some(t) => target_resolved(r, t.rsplit(':').next().unwrap_or(t)),
        None => r.clone(),
    };
    Ok(if probe(&target.endpoint).await.is_some() {
        0
    } else {
        1
    })
}

async fn list_sessions(json: bool) -> Result<i32, String> {
    let infos = ipc::session_infos().await;
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

async fn pipe(r: &Resolved, command: PipeCommands) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let text = run_pipe(
        &mut client,
        command,
        &r.session,
        r.idempotency_key.as_deref(),
    )
    .await?;
    print!("{text}");
    Ok(0)
}

async fn run_pipe(
    client: &mut Client,
    command: PipeCommands,
    session: &str,
    key: Option<&str>,
) -> Result<String, String> {
    match command {
        PipeCommands::Create { name, capacity } => pipe_create(client, &name, capacity).await,
        PipeCommands::Write { name, data } => pipe_write(client, &name, &data, key).await,
        PipeCommands::Read {
            name,
            since,
            consumer,
            wait_ms,
            limit,
            json,
        } => {
            let args = PipeReadArgs {
                since,
                consumer,
                wait_ms,
                limit,
            };
            pipe_read(client, &name, args, json).await
        }
        PipeCommands::List { json } => pipe_list(client, json, session).await,
        PipeCommands::Subscribe { name } => pipe_subscribe(client, &name).await,
    }
}

async fn pipe_create(
    client: &mut Client,
    name: &str,
    capacity: Option<usize>,
) -> Result<String, String> {
    client
        .create_pipe(name, capacity)
        .await
        .map_err(|e| format!("Pipe create error: {e}"))?;
    Ok(format!("Virtual pipe '{name}' created\n"))
}

async fn pipe_write(
    client: &mut Client,
    name: &str,
    data: &str,
    key: Option<&str>,
) -> Result<String, String> {
    apply_key(client, key);
    client
        .write_pipe(name, data)
        .await
        .map_err(|e| format!("Pipe write error: {e}"))?;
    Ok(String::new())
}

async fn pipe_read(
    client: &mut Client,
    name: &str,
    args: PipeReadArgs,
    json: bool,
) -> Result<String, String> {
    let (messages, next) = client
        .read_pipe(name, args)
        .await
        .map_err(|e| format!("Pipe read error: {e}"))?;
    render::pipe_read(&messages, next, json)
}

async fn pipe_list(client: &mut Client, json: bool, session: &str) -> Result<String, String> {
    let pipes = client
        .list_pipes()
        .await
        .map_err(|e| format!("Pipe list error: {e}"))?;
    render::pipes(&pipes, json, session)
}

async fn pipe_subscribe(client: &mut Client, name: &str) -> Result<String, String> {
    client
        .subscribe_pipe(name, |event| {
            if let Some(text) = render::pipe_event(&event) {
                print!("{text}");
            }
            true
        })
        .await
        .map_err(|e| format!("Pipe subscribe error: {e}"))?;
    Ok(String::new())
}
