use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

use super::{Node, NodeCore, NodeError, SpawnSpec};
use crate::protocol::StreamEvent;

const COLS: u16 = 80;
const ROWS: u16 = 24;

pub struct PtyNode {
    core: Arc<NodeCore>,
    stdin_tx: mpsc::UnboundedSender<Vec<u8>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl PtyNode {
    pub fn spawn(
        spec: SpawnSpec,
        event_tx: broadcast::Sender<StreamEvent>,
    ) -> Result<Self, NodeError> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| NodeError::Internal(format!("Failed to open PTY: {}", e)))?;

        let mut cmd = CommandBuilder::new(&spec.command);
        cmd.args(&spec.args);
        if let Some(dir) = &spec.cwd {
            cmd.cwd(dir);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| NodeError::Internal(format!("Failed to spawn command in PTY: {}", e)))?;
        let pid = child.process_id();
        let killer = child.clone_killer();
        // The parent must drop its slave fd so that child exit surfaces as EOF on the master.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| NodeError::Internal(format!("Failed to clone PTY reader: {}", e)))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| NodeError::Internal(format!("Failed to take PTY writer: {}", e)))?;

        let core = Arc::new(NodeCore::new(
            &spec,
            true,
            pid,
            usize::from(COLS),
            usize::from(ROWS),
        ));
        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        spawn_writer(writer, stdin_rx);

        let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
        {
            let core = Arc::clone(&core);
            let replies = stdin_tx.clone();
            std::thread::spawn(move || {
                pump_output(reader, &core, &event_tx, &replies);
                let _ = eof_tx.send(());
            });
        }
        {
            let core = Arc::clone(&core);
            std::thread::spawn(move || {
                let code = child.wait().ok().map(|s| s.exit_code() as i32);
                // Give the reader a moment to drain output the child wrote before exiting.
                let _ = eof_rx.recv_timeout(Duration::from_millis(500));
                core.mark_exited(code);
            });
        }

        Ok(Self {
            core,
            stdin_tx,
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(killer),
        })
    }
}

fn spawn_writer(mut writer: Box<dyn Write + Send>, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    std::thread::spawn(move || {
        while let Some(data) = rx.blocking_recv() {
            if writer
                .write_all(&data)
                .and_then(|()| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });
}

fn pump_output(
    mut reader: Box<dyn Read + Send>,
    core: &NodeCore,
    events: &broadcast::Sender<StreamEvent>,
    replies: &mpsc::UnboundedSender<Vec<u8>>,
) {
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let bytes = &chunk[..n];
                for reply in core.ingest(bytes) {
                    let _ = replies.send(reply);
                }
                let _ = events.send(StreamEvent::Output {
                    name: core.name.clone(),
                    data: String::from_utf8_lossy(bytes).into_owned(),
                });
            }
        }
    }
}

impl Node for PtyNode {
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
        if !self.is_running() {
            return Ok(());
        }
        let mut killer = self.killer.lock().unwrap_or_else(|e| e.into_inner());
        killer.kill().map_err(NodeError::Io)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), NodeError> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if let Some(master) = self
            .master
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        self.resize_screen(cols, rows);
        Ok(())
    }

    fn foreground_pid(&self) -> Option<u32> {
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner());
        let leader = master.as_ref()?.process_group_leader()?;
        u32::try_from(leader).ok().filter(|p| *p > 0)
    }

    fn release(&self) {
        self.master.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
