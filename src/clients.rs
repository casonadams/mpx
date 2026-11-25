//! Attached clients (tmux clients): the window-size policy that sizes a session's layout for all of
//! them, the names a client is known by, and the lookup of clients across the sessions of one socket
//! directory.
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::client::Client;
use crate::config::{MAX_WINDOW_SIZE, Size, WindowSize};
use crate::format;
use crate::ipc;
use crate::protocol::ClientInfo;

/// Refuses a width or height outside `1..=MAX_WINDOW_SIZE`.
pub fn check_size(width: u16, height: u16) -> Result<(), String> {
    if (1..=MAX_WINDOW_SIZE).contains(&width) && (1..=MAX_WINDOW_SIZE).contains(&height) {
        return Ok(());
    }
    Err(format!(
        "size {width}x{height} out of range (1 to {MAX_WINDOW_SIZE})"
    ))
}

/// The size a session's window is laid out for. Clients without a known size (zero) are ignored;
/// with none left the window keeps `requested`, the size its caller asked for. `manual` ignores the
/// clients and uses `default`.
pub fn effective_size(
    policy: WindowSize,
    default: Size,
    clients: &[ClientInfo],
    requested: (u16, u16),
) -> (u16, u16) {
    let sized: Vec<&ClientInfo> = clients
        .iter()
        .filter(|c| c.width > 0 && c.height > 0)
        .collect();
    match policy {
        WindowSize::Manual => (default.width, default.height),
        _ if sized.is_empty() => requested,
        WindowSize::Latest => sized
            .iter()
            .max_by_key(|c| c.activity_ms)
            .map_or(requested, |c| (c.width, c.height)),
        WindowSize::Smallest => (
            sized.iter().map(|c| c.width).min().unwrap_or(requested.0),
            sized.iter().map(|c| c.height).min().unwrap_or(requested.1),
        ),
        WindowSize::Largest => (
            sized.iter().map(|c| c.width).max().unwrap_or(requested.0),
            sized.iter().map(|c| c.height).max().unwrap_or(requested.1),
        ),
    }
}

static CLIENT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Name for a client started in this process: its terminal's device path when stdin is a terminal,
/// else a name unique within the process.
pub fn terminal_name() -> String {
    #[cfg(unix)]
    if let Some(path) = tty_path(0) {
        return path;
    }
    format!(
        "client-{}-{}",
        std::process::id(),
        CLIENT_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// The process id of this process's parent, the shell a client was started from: `attach -x` hangs
/// it up. None on Windows, where there is no such signal.
#[cfg(unix)]
pub fn parent_pid() -> Option<u32> {
    // SAFETY: getppid(2) only reads the process table entry of this process.
    u32::try_from(unsafe { libc::getppid() }).ok()
}

#[cfg(not(unix))]
pub fn parent_pid() -> Option<u32> {
    None
}

#[cfg(unix)]
fn tty_path(fd: i32) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
    let rc = unsafe { libc::ttyname_r(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

/// An attached client and the endpoint of the daemon of its session.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientRow {
    pub endpoint: String,
    pub info: ClientInfo,
}

/// Format `list-clients` uses when none is given.
pub const DEFAULT_CLIENT_FORMAT: &str = "#{client_name}: #{client_session} [#{client_width}x#{client_height}]#{?client_readonly, (readonly),}";

const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// The clients of every running session in `dir`, in session name order.
pub async fn attached_clients(dir: &Path) -> Vec<ClientRow> {
    let probes = ipc::list_sessions(dir).into_iter().map(|ep| async move {
        let probe = async {
            let mut client = Client::connect(&ep.endpoint).await.ok()?;
            client.list_clients().await.ok()
        };
        let clients = tokio::time::timeout(PROBE_TIMEOUT, probe)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        clients
            .into_iter()
            .map(|info| ClientRow {
                endpoint: ep.endpoint.clone(),
                info,
            })
            .collect::<Vec<_>>()
    });
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// The row of the client called `name`.
pub fn find_row<'a>(rows: &'a [ClientRow], name: &str) -> Option<&'a ClientRow> {
    rows.iter().find(|r| r.info.name == name)
}

/// The most recently active client among `rows`.
pub fn latest_row(rows: &[ClientRow]) -> Option<&ClientRow> {
    rows.iter().max_by_key(|r| r.info.activity_ms)
}

/// The rows of the clients attached to `session`.
pub fn session_rows<'a>(rows: &'a [ClientRow], session: &str) -> Vec<&'a ClientRow> {
    rows.iter().filter(|r| r.info.session == session).collect()
}

/// `list-clients` lines: one per client, `format` expanded for it. `session_attached` is how many
/// of `rows` belong to the client's session.
pub fn client_lines(rows: &[ClientRow], format: &str) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let attached = session_rows(rows, &row.info.session).len();
            format::expand(format, &format::client_vars(&row.info, attached))
        })
        .collect()
}
