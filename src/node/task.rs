use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};

use super::{Node, NodeError};
use crate::buffer::RingBuffer;
use crate::protocol::{NodeInfo, StreamEvent};
use crate::vt::VtScreen;

pub struct TaskNode {
    name: String,
    command: String,
    pid: Option<u32>,
    buffer: Arc<RwLock<RingBuffer>>,
    screen: Arc<RwLock<VtScreen>>,
    stdin: Option<Arc<Mutex<tokio::process::ChildStdin>>>,
    child_handle: Option<Arc<Mutex<tokio::process::Child>>>,
    exit_code: Arc<RwLock<Option<i32>>>,
    exit_notify: Arc<Notify>,
    running: Arc<AtomicBool>,
}

impl TaskNode {
    pub fn spawn(
        name: String,
        cmd: String,
        args: &[String],
        cwd: Option<&str>,
        event_tx: broadcast::Sender<StreamEvent>,
    ) -> Result<Self, NodeError> {
        let mut command = Command::new(&cmd);
        command.args(args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn()?;
        let pid = child.id();

        let stdin = child.stdin.take().map(|s| Arc::new(Mutex::new(s)));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let buffer = Arc::new(RwLock::new(RingBuffer::default()));
        let screen = Arc::new(RwLock::new(VtScreen::new(80, 24)));
        let exit_code = Arc::new(RwLock::new(None));
        let exit_notify = Arc::new(Notify::new());
        let running = Arc::new(AtomicBool::new(true));

        let display_cmd = if args.is_empty() {
            cmd.clone()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };

        // Pipe reader for stdout
        if let Some(mut out) = stdout {
            let buf_clone = Arc::clone(&buffer);
            let tx_clone = event_tx.clone();
            let screen_clone = Arc::clone(&screen);
            let node_name = name.clone();
            tokio::spawn(async move {
                let mut chunk = [0u8; 4096];
                loop {
                    match out.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let bytes = &chunk[..n];
                            {
                                let mut b = buf_clone.write().await;
                                b.write(bytes);
                            }
                            {
                                let mut sc = screen_clone.write().await;
                                sc.feed(bytes);
                            }
                            let text = String::from_utf8_lossy(bytes).into_owned();
                            let _ = tx_clone.send(StreamEvent::Output {
                                name: node_name.clone(),
                                data: text,
                            });
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        // Pipe reader for stderr
        if let Some(mut err) = stderr {
            let buf_clone = Arc::clone(&buffer);
            let tx_clone = event_tx.clone();
            let node_name = name.clone();
            let screen_clone = Arc::clone(&screen);
            tokio::spawn(async move {
                let mut chunk = [0u8; 4096];
                loop {
                    match err.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let bytes = &chunk[..n];
                            {
                                let mut b = buf_clone.write().await;
                                b.write(bytes);
                            }
                            let text = String::from_utf8_lossy(bytes).into_owned();
                            {
                                let mut sc = screen_clone.write().await;
                                sc.feed(bytes);
                            }
                            let _ = tx_clone.send(StreamEvent::Output {
                                name: node_name.clone(),
                                data: text,
                            });
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        // Wait on process exit
        let child_arc = Arc::new(Mutex::new(child));
        let child_for_wait = Arc::clone(&child_arc);
        let exit_clone = Arc::clone(&exit_code);
        let notify_clone = Arc::clone(&exit_notify);
        let running_clone = Arc::clone(&running);
        let tx_exit = event_tx.clone();
        let exit_name = name.clone();

        tokio::spawn(async move {
            let mut child = child_for_wait.lock().await;
            let status = child.wait().await;
            let code = status.ok().and_then(|s| s.code());
            {
                let mut ec = exit_clone.write().await;
                *ec = code;
            }
            running_clone.store(false, Ordering::SeqCst);
            notify_clone.notify_waiters();
            let _ = tx_exit.send(StreamEvent::Exit {
                name: exit_name,
                code,
            });
        });

        Ok(Self {
            name,
            command: display_cmd,
            pid,
            buffer,
            screen,
            stdin,
            child_handle: Some(child_arc),
            exit_code,
            exit_notify,
            running,
        })
    }
}

impl Node for TaskNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn info(&self) -> NodeInfo {
        let is_running = self.running.load(Ordering::SeqCst);
        let status = if is_running {
            "running".to_string()
        } else {
            let code = self.exit_code.try_read().ok().and_then(|c| *c);
            match code {
                Some(c) => format!("exited({})", c),
                None => "exited".to_string(),
            }
        };

        NodeInfo {
            name: self.name.clone(),
            pid: self.pid,
            pty: false,
            command: self.command.clone(),
            status,
        }
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(NodeError::NotRunning);
        }
        let stdin = self.stdin.as_ref().ok_or(NodeError::NotRunning)?;
        let stdin = Arc::clone(stdin);
        let data = data.to_vec();

        tokio::spawn(async move {
            let mut writer = stdin.lock().await;
            let _ = writer.write_all(&data).await;
            let _ = writer.flush().await;
        });

        Ok(())
    }

    fn read_buffer(&self, tail: Option<usize>) -> String {
        let buf = self.buffer.try_read();
        match buf {
            Ok(b) => match tail {
                Some(n) => b.tail_lines(n),
                None => b.snapshot_str(),
            },
            Err(_) => String::new(),
        }
    }
    fn read_screen(&self) -> String {
        self.screen
            .try_read()
            .map(|s| s.snapshot())
            .unwrap_or_default()
    }


    fn kill(&self) -> Result<(), NodeError> {
        if let Some(child_arc) = &self.child_handle {
            let child_clone = Arc::clone(child_arc);
            tokio::spawn(async move {
                let mut child = child_clone.lock().await;
                let _ = child.kill().await;
            });
        }
        Ok(())
    }

    fn wait_exit<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<i32>> + Send + 'a>> {
        Box::pin(async move {
            if !self.running.load(Ordering::SeqCst) {
                return *self.exit_code.read().await;
            }
            self.exit_notify.notified().await;
            *self.exit_code.read().await
        })
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
