//! tmux `pipe-pane`: a shell command fed with a pane's output until the pane or the command ends.
use std::collections::HashMap;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::task::JoinHandle;

use crate::node::pane::PaneHandle;
use crate::node::sink::Sink;
use crate::protocol::StreamEvent;

#[derive(Default)]
pub(super) struct Pipes(HashMap<String, JoinHandle<()>>);

impl Pipes {
    fn running(&self, pane: &str) -> bool {
        self.0.get(pane).is_some_and(|task| !task.is_finished())
    }

    /// Stops the pipe of `pane`, if any.
    pub(super) fn close(&mut self, pane: &str) {
        if let Some(task) = self.0.remove(pane) {
            task.abort();
        }
    }

    /// Starts piping `pane` into `command`, replacing an earlier pipe. With `toggle`, a pipe that is
    /// already running is stopped and nothing new starts.
    pub(super) fn open(
        &mut self,
        pane: &str,
        handle: &PaneHandle,
        command: &str,
        toggle: bool,
    ) -> Result<(), String> {
        let was_running = self.running(pane);
        self.close(pane);
        if toggle && was_running {
            return Ok(());
        }
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to run '{command}': {e}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "pipe has no input".to_string())?;
        let (sink, mut events) = Sink::channel(None);
        handle.attach(None, sink);
        let task = tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    StreamEvent::Output { data, .. } => {
                        if stdin.write_all(data.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    StreamEvent::Exit { .. } => break,
                    _ => {}
                }
            }
            drop(stdin);
            let _ = child.wait().await;
        });
        self.0.insert(pane.to_string(), task);
        Ok(())
    }
}
