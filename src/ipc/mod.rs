#[cfg(unix)]
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncWrite};

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncStream for T {}

pub fn default_endpoint(session: &str) -> String {
    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(runtime_dir).join("mpx").join(format!("{}.sock", session));
            return path.to_string_lossy().into_owned();
        }
        let uid = unsafe { libc::getuid() };
        let dir = format!("/tmp/mpx-{}", uid);
        let path = PathBuf::from(dir).join(format!("{}.sock", session));
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    {
        format!(r"\\.\pipe\mpx-{}", session)
    }
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
            std::fs::create_dir_all(parent)?;
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
pub async fn connect(endpoint: &str) -> std::io::Result<IpcStream> {
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

    pub async fn accept(&mut self) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        self.server.connect().await?;
        let next_server = tokio::net::windows::named_pipe::ServerOptions::new()
            .create(&self.pipe_name)?;
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
pub async fn connect(endpoint: &str) -> std::io::Result<IpcStream> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)
}
