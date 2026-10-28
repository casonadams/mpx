#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
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

pub fn default_endpoint(session: &str) -> String {
    #[cfg(unix)]
    {
        socket_dir()
            .join(format!("{}.sock", session))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\mpx-{}", session)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEndpoint {
    pub name: String,
    pub endpoint: String,
}

/// Session endpoints discovered in the socket directory (or `\\.\pipe\` on Windows), sorted by name.
/// Liveness is not checked.
pub fn list_sessions() -> Vec<SessionEndpoint> {
    sessions_from_names(entry_names())
}

/// Maps directory entry names to session endpoints: unique, sorted, non-session entries dropped.
pub fn sessions_from_names<I, S>(names: I) -> Vec<SessionEndpoint>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let sessions: std::collections::BTreeSet<String> = names
        .into_iter()
        .filter_map(|n| session_name(n.as_ref()).map(str::to_string))
        .collect();
    sessions
        .into_iter()
        .map(|name| SessionEndpoint {
            endpoint: default_endpoint(&name),
            name,
        })
        .collect()
}

#[cfg(unix)]
fn session_name(entry: &str) -> Option<&str> {
    entry.strip_suffix(".sock").filter(|n| !n.is_empty())
}

#[cfg(windows)]
fn session_name(entry: &str) -> Option<&str> {
    entry.strip_prefix("mpx-").filter(|n| !n.is_empty())
}

fn entry_names() -> Vec<String> {
    #[cfg(unix)]
    let dir = socket_dir();
    #[cfg(windows)]
    let dir = PathBuf::from(r"\\.\pipe\");
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
    if std::fs::metadata(dir)?.uid() == unsafe { libc::getuid() } {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
pub struct IpcListener {
    listener: tokio::net::UnixListener,
    path: PathBuf,
}

#[cfg(unix)]
impl IpcListener {
    pub fn bind(endpoint: &str) -> std::io::Result<Self> {
        let path = PathBuf::from(endpoint);
        if let Some(parent) = path.parent() {
            secure_dir(parent)?;
        }
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let listener = tokio::net::UnixListener::bind(&path)?;
        Ok(Self { listener, path })
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
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
pub type IpcStream = tokio::net::UnixStream;

#[cfg(unix)]
pub async fn connect_stream(endpoint: &str) -> std::io::Result<IpcStream> {
    tokio::net::UnixStream::connect(endpoint).await
}

#[cfg(windows)]
pub struct IpcListener {
    pipe_name: String,
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl IpcListener {
    pub fn bind(endpoint: &str) -> std::io::Result<Self> {
        let pipe_name = endpoint.to_string();
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
pub type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(windows)]
pub async fn connect_stream(endpoint: &str) -> std::io::Result<IpcStream> {
    for _ in 0..50 {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(231) => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
    tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)
}

/// Info of every session that answers within a short timeout, sorted by name.
pub async fn session_infos() -> Vec<crate::protocol::SessionInfo> {
    let mut out = Vec::new();
    for ep in list_sessions() {
        let probe = async {
            let mut client = crate::client::Client::connect(&ep.endpoint).await.ok()?;
            client.info().await.ok()
        };
        if let Ok(Some(info)) = tokio::time::timeout(SESSION_PROBE_TIMEOUT, probe).await {
            out.push(info);
        }
    }
    out
}

const SESSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
