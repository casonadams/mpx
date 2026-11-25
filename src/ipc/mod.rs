use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncWrite};

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncStream for T {}

/// Directory holding session sockets (unix) or mpx support files (elsewhere).
pub fn socket_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(runtime_dir).join("mpx");
        }
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/mpx-{}", uid))
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join("mpx")
    }
}

/// Endpoint of `session` whose sockets live in `dir`.
pub fn endpoint_in(dir: &Path, session: &str) -> String {
    #[cfg(unix)]
    {
        dir.join(format!("{}.sock", session))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(windows)]
    {
        if dir == socket_dir() {
            format!(r"\\.\pipe\mpx-{}", session)
        } else {
            let hash = fnv1a_64(dir.to_string_lossy().as_bytes());
            format!(r"\\.\pipe\mpxd-{hash:08x}-{}", session)
        }
    }
}

/// Fails when `endpoint` is too long for a unix socket address (`sun_path`, 104 bytes on macOS and
/// 108 elsewhere, NUL included).
#[cfg(unix)]
pub fn check_endpoint(endpoint: &str) -> Result<(), String> {
    const SUN_PATH_LEN: usize = if cfg!(target_os = "macos") { 104 } else { 108 };
    if endpoint.len() < SUN_PATH_LEN {
        return Ok(());
    }
    Err(format!(
        "socket path {endpoint} is {} bytes; the limit is {} (use a shorter session name)",
        endpoint.len(),
        SUN_PATH_LEN - 1
    ))
}

#[cfg(windows)]
pub fn check_endpoint(_endpoint: &str) -> Result<(), String> {
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEndpoint {
    pub name: String,
    pub endpoint: String,
}

/// Session endpoints found in `dir` (the pipe namespace on Windows), sorted by name.
/// Liveness is not checked.
pub fn list_sessions(dir: &Path) -> Vec<SessionEndpoint> {
    sessions_from_names(dir, entry_names(dir))
}

/// Maps directory entry names to session endpoints in `dir`: unique, sorted, non-session entries dropped.
pub fn sessions_from_names<I, S>(dir: &Path, names: I) -> Vec<SessionEndpoint>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let sessions: std::collections::BTreeSet<String> = names
        .into_iter()
        .filter_map(|n| session_name(dir, n.as_ref()).map(str::to_string))
        .collect();
    sessions
        .into_iter()
        .map(|name| SessionEndpoint {
            endpoint: endpoint_in(dir, &name),
            name,
        })
        .collect()
}

#[cfg(unix)]
fn session_name<'a>(_dir: &Path, entry: &'a str) -> Option<&'a str> {
    entry.strip_suffix(".sock").filter(|n| !n.is_empty())
}

#[cfg(windows)]
fn session_name<'a>(dir: &Path, entry: &'a str) -> Option<&'a str> {
    let name = if dir == socket_dir() {
        entry.strip_prefix("mpx-")?
    } else {
        let hash = fnv1a_64(dir.to_string_lossy().as_bytes());
        entry.strip_prefix(&format!("mpxd-{hash:08x}-"))?
    };
    if name.is_empty() { None } else { Some(name) }
}

fn entry_names(dir: &Path) -> Vec<String> {
    #[cfg(windows)]
    let dir = {
        let _ = dir;
        Path::new(r"\\.\pipe\")
    };
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
fn secure_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() || meta.uid() != unsafe { libc::getuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a directory owned by this user", dir.display()),
        ));
    }
    if meta.mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
pub struct IpcListener {
    listener: tokio::net::UnixListener,
    path: PathBuf,
    /// Device and inode of the socket file this listener created.
    id: (u64, u64),
}

#[cfg(unix)]
impl IpcListener {
    pub fn bind(endpoint: &str) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let path = PathBuf::from(endpoint);
        if let Some(parent) = path.parent() {
            secure_dir(parent)?;
        }
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("a daemon is already listening on {endpoint}"),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = tokio::net::UnixListener::bind(&path)?;
        let meta = std::fs::symlink_metadata(&path)?;
        Ok(Self {
            listener,
            path,
            id: (meta.dev(), meta.ino()),
        })
    }

    pub async fn accept(&self) -> std::io::Result<tokio::net::UnixStream> {
        let (stream, _) = self.listener.accept().await?;
        Ok(stream)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for IpcListener {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        let still_ours =
            std::fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.id);
        if still_ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
pub type IpcStream = tokio::net::UnixStream;

#[cfg(unix)]
pub async fn connect_stream(endpoint: &str) -> std::io::Result<IpcStream> {
    tokio::net::UnixStream::connect(endpoint).await
}

#[cfg(windows)]
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3u64);
    }
    hash
}

#[cfg(windows)]
fn pipe_name(endpoint: &str) -> String {
    if endpoint.starts_with(r"\\.\pipe\") {
        endpoint.to_string()
    } else {
        let hash = fnv1a_64(endpoint.as_bytes());
        let sanitized: String = endpoint
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let tail = if sanitized.len() > 32 {
            &sanitized[sanitized.len() - 32..]
        } else {
            &sanitized
        };
        format!(r"\\.\pipe\mpx-{hash:016x}-{tail}")
    }
}

#[cfg(windows)]
pub struct IpcListener {
    pipe_name: String,
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl IpcListener {
    pub fn bind(endpoint: &str) -> std::io::Result<Self> {
        let pipe_name = pipe_name(endpoint);
        let server = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)?;
        Ok(Self { pipe_name, server })
    }

    pub async fn accept(
        &mut self,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        self.server.connect().await?;
        let next_server =
            tokio::net::windows::named_pipe::ServerOptions::new().create(&self.pipe_name)?;
        let connected = std::mem::replace(&mut self.server, next_server);
        Ok(connected)
    }

    pub fn endpoint(&self) -> &str {
        &self.pipe_name
    }
}
#[cfg(windows)]
impl Drop for IpcListener {
    fn drop(&mut self) {
        let _ = self.server.disconnect();
    }
}

#[cfg(windows)]
pub type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(windows)]
pub async fn connect_stream(endpoint: &str) -> std::io::Result<IpcStream> {
    let pipe_name = pipe_name(endpoint);
    for _ in 0..50 {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(&pipe_name) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(231) => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
    tokio::net::windows::named_pipe::ClientOptions::new().open(&pipe_name)
}

/// Info of every session in `dir` that answers within a short timeout, sorted by name.
pub async fn session_infos(dir: &Path) -> Vec<crate::protocol::SessionInfo> {
    let probes = list_sessions(dir).into_iter().map(|ep| async move {
        let probe = async {
            let mut client = crate::client::Client::connect(&ep.endpoint).await.ok()?;
            client.info().await.ok()
        };
        tokio::time::timeout(SESSION_PROBE_TIMEOUT, probe)
            .await
            .ok()
            .flatten()
    });
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect()
}

const SESSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
