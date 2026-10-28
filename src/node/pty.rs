use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use super::pane::PaneMsg;
use super::{NodeError, Process, SpawnSpec};

const COLS: u16 = 80;
const ROWS: u16 = 24;

type Master = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

pub struct PtyNode {
    stdin_tx: mpsc::UnboundedSender<Vec<u8>>,
    master: Master,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
}

/// Terminal-side control of a PTY (window size, foreground process), held by its pane actor.
#[derive(Clone)]
pub struct PtyTerminal {
    master: Master,
}

impl PtyTerminal {
    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(master) = self.master.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = master.resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Process group leader of the terminal, i.e. the program the user is running in it.
    pub fn foreground_pid(&self) -> Option<u32> {
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner());
        let leader = master.as_ref()?.process_group_leader()?;
        u32::try_from(leader).ok().filter(|p| *p > 0)
    }
}

impl PtyNode {
    /// Starts the child on a PTY. Output and its exit are sent to `out`.
    pub fn spawn(spec: &SpawnSpec, out: mpsc::UnboundedSender<PaneMsg>) -> Result<Self, NodeError> {
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
        // The slave stays open until the child is reaped: closing it first can discard output the
        // reader has not consumed yet. The waiter releases it once the child has exited.
        let slave = pair.slave;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| NodeError::Internal(format!("Failed to clone PTY reader: {}", e)))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| NodeError::Internal(format!("Failed to take PTY writer: {}", e)))?;

        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        spawn_writer(writer, stdin_rx);

        let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
        {
            let out = out.clone();
            std::thread::spawn(move || {
                pump_output(reader, &out);
                let _ = eof_tx.send(());
            });
        }
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code() as i32);
            drop(slave);
            // EOF follows the release of the slave once the reader has drained every byte.
            let _ = eof_rx.recv_timeout(Duration::from_millis(500));
            let _ = out.send(PaneMsg::Exited(code));
        });

        Ok(Self {
            stdin_tx,
            master: Arc::new(Mutex::new(Some(pair.master))),
            killer: Mutex::new(killer),
            pid,
        })
    }

    pub fn terminal(&self) -> PtyTerminal {
        PtyTerminal {
            master: Arc::clone(&self.master),
        }
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

fn pump_output(mut reader: Box<dyn Read + Send>, out: &mpsc::UnboundedSender<PaneMsg>) {
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.send(PaneMsg::Output(chunk[..n].to_vec())).is_err() {
                    break;
                }
            }
        }
    }
}

impl Process for PtyNode {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
        self.stdin_tx
            .send(data.to_vec())
            .map_err(|_| NodeError::NotRunning)
    }

    fn kill(&self) -> Result<(), NodeError> {
        let mut killer = self.killer.lock().unwrap_or_else(|e| e.into_inner());
        killer.kill().map_err(NodeError::Io)
    }
}
