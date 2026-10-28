use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{Node, NodeCore, NodeError, SpawnSpec};
use crate::protocol::StreamEvent;

pub struct TaskNode {
    core: Arc<NodeCore>,
    stdin_tx: mpsc::UnboundedSender<Vec<u8>>,
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl TaskNode {
    pub fn spawn(
        spec: SpawnSpec,
        event_tx: broadcast::Sender<StreamEvent>,
    ) -> Result<Self, NodeError> {
        let mut command = Command::new(&spec.command);
        command.args(&spec.args);
        if let Some(dir) = &spec.cwd {
            command.current_dir(dir);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        let mut child = command.spawn()?;
        let pid = child.id();
        let core = Arc::new(NodeCore::new(&spec, false, pid, 80, 24));

        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        if let Some(stdin) = child.stdin.take() {
            tokio::spawn(write_stdin(stdin, stdin_rx));
        }

        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(spawn_reader(out, Arc::clone(&core), event_tx.clone()));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(spawn_reader(err, Arc::clone(&core), event_tx));
        }

        let (kill_tx, kill_rx) = oneshot::channel();
        tokio::spawn(wait_child(child, kill_rx, readers, Arc::clone(&core)));

        Ok(Self {
            core,
            stdin_tx,
            kill_tx: Mutex::new(Some(kill_tx)),
        })
    }
}

async fn write_stdin(
    mut stdin: tokio::process::ChildStdin,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(data) = rx.recv().await {
        if stdin.write_all(&data).await.is_err() || stdin.flush().await.is_err() {
            break;
        }
    }
}

fn spawn_reader<R>(
    mut pipe: R,
    core: Arc<NodeCore>,
    events: broadcast::Sender<StreamEvent>,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let bytes = &chunk[..n];
                    core.ingest(bytes);
                    let _ = events.send(StreamEvent::Output {
                        name: core.name.clone(),
                        data: String::from_utf8_lossy(bytes).into_owned(),
                    });
                }
            }
        }
    })
}

async fn wait_child(
    mut child: Child,
    kill_rx: oneshot::Receiver<()>,
    readers: Vec<JoinHandle<()>>,
    core: Arc<NodeCore>,
) {
    let kill_signal = async move {
        if kill_rx.await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        () = kill_signal => {
            let _ = child.start_kill();
            child.wait().await.ok()
        }
    };
    let code = status.and_then(|s| s.code());
    for reader in readers {
        let _ = tokio::time::timeout(Duration::from_millis(500), reader).await;
    }
    core.mark_exited(code);
}

impl Node for TaskNode {
    fn core(&self) -> &NodeCore {
        &self.core
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
        if !self.is_running() {
            return Err(NodeError::NotRunning);
        }
        self.stdin_tx
            .send(data.to_vec())
            .map_err(|_| NodeError::NotRunning)
    }

    fn kill(&self) -> Result<(), NodeError> {
        if let Some(tx) = self
            .kill_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = tx.send(());
        }
        Ok(())
    }
}
