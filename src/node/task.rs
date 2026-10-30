use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::pacing::Inflight;
use super::pane::PaneMsg;
use super::{NodeError, Process, SpawnSpec};

pub struct TaskNode {
    stdin_tx: mpsc::UnboundedSender<Vec<u8>>,
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
    pid: Option<u32>,
}

impl TaskNode {
    /// Starts the command with piped stdio. Output and its exit are sent to `out`.
    pub fn spawn(
        spec: &SpawnSpec,
        out: mpsc::UnboundedSender<PaneMsg>,
        inflight: Arc<Inflight>,
    ) -> Result<Self, NodeError> {
        let mut command = Command::new(&spec.command);
        command.args(&spec.args);
        if let Some(dir) = &spec.cwd {
            command.current_dir(dir);
        }
        command.env("TERM", &spec.term);
        command.env("COLORTERM", "truecolor");
        command.env_remove("TERM_SESSION_ID");
        command.env("SHELL_SESSIONS_DISABLE", "1");
        command.envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        let mut child = command.spawn()?;
        let pid = child.id();

        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        if let Some(stdin) = child.stdin.take() {
            tokio::spawn(write_stdin(stdin, stdin_rx));
        }

        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(spawn_reader(stdout, out.clone(), Arc::clone(&inflight)));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(spawn_reader(stderr, out.clone(), inflight));
        }

        let (kill_tx, kill_rx) = oneshot::channel();
        tokio::spawn(wait_child(child, kill_rx, readers, out));

        Ok(Self {
            stdin_tx,
            kill_tx: Mutex::new(Some(kill_tx)),
            pid,
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
    out: mpsc::UnboundedSender<PaneMsg>,
    inflight: Arc<Inflight>,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        loop {
            inflight.wait_for_room_async().await;
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    inflight.add();
                    if out.send(PaneMsg::Output(chunk[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

async fn wait_child(
    mut child: Child,
    kill_rx: oneshot::Receiver<()>,
    readers: Vec<JoinHandle<()>>,
    out: mpsc::UnboundedSender<PaneMsg>,
) {
    let kill_signal = async move {
        if kill_rx.await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let status = tokio::select! {
        status = child.wait() => status.ok().and_then(|s| s.code()),
        () = kill_signal => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            None
        }
    };
    let code = status;
    for reader in readers {
        let _ = tokio::time::timeout(Duration::from_millis(500), reader).await;
    }
    let _ = out.send(PaneMsg::Exited(code));
}

impl Process for TaskNode {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
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
