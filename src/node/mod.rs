pub mod agent;
pub mod process;
pub mod pty;
pub mod task;

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::watch;

use crate::buffer::RingBuffer;
use crate::keys::TermModes;
use crate::protocol::{NodeInfo, NodeState, PaneId};
use crate::vt::{Alert, VtScreen};

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Process not running")]
    NotRunning,
    #[error("Failed to write to stdin: {0}")]
    StdinFailed(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Everything needed to start a node process.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub name: String,
    pub id: PaneId,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// Extra environment, applied after the terminal defaults.
    pub env: Vec<(String, String)>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Exit state: `None` while running, `Some(code)` once exited.
type ExitState = Option<Option<i32>>;

/// State shared by every node kind: output retention, screen emulation and exit tracking.
pub struct NodeCore {
    pub name: String,
    pub id: PaneId,
    pub command: String,
    pub pty: bool,
    pub pid: Option<u32>,
    pub spawn_cwd: Option<String>,
    pub started_ms: u64,
    buffer: Mutex<RingBuffer>,
    screen: Mutex<VtScreen>,
    last_output_ms: AtomicU64,
    exit_tx: watch::Sender<ExitState>,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl NodeCore {
    pub fn new(spec: &SpawnSpec, pty: bool, pid: Option<u32>, cols: usize, rows: usize) -> Self {
        let (exit_tx, _) = watch::channel(None);
        let started_ms = now_ms();
        let command = if spec.args.is_empty() {
            spec.command.clone()
        } else {
            format!("{} {}", spec.command, spec.args.join(" "))
        };
        Self {
            name: spec.name.clone(),
            id: spec.id,
            command,
            pty,
            pid,
            spawn_cwd: spec.cwd.clone(),
            started_ms,
            buffer: Mutex::new(RingBuffer::default()),
            screen: Mutex::new(VtScreen::new(cols, rows)),
            last_output_ms: AtomicU64::new(started_ms),
            exit_tx,
        }
    }

    /// Records output and returns the terminal replies the screen wants written back.
    pub fn ingest(&self, bytes: &[u8]) -> Vec<Vec<u8>> {
        locked(&self.buffer).write(bytes);
        self.last_output_ms.store(now_ms(), Ordering::SeqCst);
        let mut screen = locked(&self.screen);
        screen.feed(bytes);
        screen.take_responses()
    }

    pub fn exit_state(&self) -> ExitState {
        *self.exit_tx.borrow()
    }

    pub fn mark_exited(&self, code: Option<i32>) {
        self.exit_tx.send_replace(Some(code));
    }

    pub fn last_output_ms(&self) -> u64 {
        self.last_output_ms.load(Ordering::SeqCst)
    }

    pub fn buffer(&self) -> MutexGuard<'_, RingBuffer> {
        locked(&self.buffer)
    }

    pub fn screen(&self) -> MutexGuard<'_, VtScreen> {
        locked(&self.screen)
    }

    pub fn exit_watch(&self) -> watch::Receiver<ExitState> {
        self.exit_tx.subscribe()
    }
}

pub trait Node: Send + Sync {
    fn core(&self) -> &NodeCore;

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError>;

    fn kill(&self) -> Result<(), NodeError>;

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), NodeError> {
        Ok(())
    }

    /// Process group leader of the PTY, when the node has a terminal.
    fn foreground_pid(&self) -> Option<u32> {
        None
    }

    /// Drops terminal handles once the process has exited.
    fn release(&self) {}

    fn name(&self) -> &str {
        &self.core().name
    }

    fn id(&self) -> PaneId {
        self.core().id
    }

    fn is_pty(&self) -> bool {
        self.core().pty
    }

    fn command(&self) -> &str {
        &self.core().command
    }

    fn pid(&self) -> Option<u32> {
        self.core().pid
    }

    fn started_ms(&self) -> u64 {
        self.core().started_ms
    }

    fn last_output_ms(&self) -> u64 {
        self.core().last_output_ms()
    }

    fn is_running(&self) -> bool {
        self.core().exit_state().is_none()
    }

    fn exit_code(&self) -> Option<i32> {
        self.core().exit_state().flatten()
    }

    /// Live working directory of the foreground process, else of the spawned process.
    fn cwd(&self) -> Option<String> {
        let core = self.core();
        self.foreground_pid()
            .or(core.pid)
            .and_then(process::cwd_of)
            .or_else(|| core.spawn_cwd.clone())
    }

    fn title(&self) -> Option<String> {
        self.core().screen().title().map(str::to_string)
    }

    fn modes(&self) -> TermModes {
        self.core().screen().modes()
    }

    fn take_alerts(&self) -> Vec<Alert> {
        self.core().screen().take_alerts()
    }

    fn read_buffer(&self, tail: Option<usize>) -> String {
        let buf = self.core().buffer();
        match tail {
            Some(n) => buf.tail_lines(n),
            None => buf.snapshot_str(),
        }
    }

    /// Total bytes ever output; the offset `read_since` resumes from.
    fn total_written(&self) -> u64 {
        self.core().buffer().total_written()
    }

    fn read_since(&self, offset: u64) -> (Vec<u8>, u64) {
        self.core().buffer().since(offset)
    }

    /// Scrollback plus screen for PTYs; the output buffer for task nodes.
    fn scrollback_text(&self, tail: Option<usize>) -> String {
        if self.is_pty() {
            self.core().screen().scrollback_text(tail)
        } else {
            self.read_buffer(tail)
        }
    }

    fn read_screen(&self) -> String {
        self.core().screen().snapshot()
    }

    fn resize_screen(&self, cols: u16, rows: u16) {
        let cols = usize::from(cols.max(1));
        let rows = usize::from(rows.max(1));
        self.core().screen().grid_mut().resize(cols, rows);
    }

    /// Resolves once the process has exited, with its exit code.
    fn wait_exit<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<i32>> + Send + 'a>> {
        Box::pin(async move {
            let mut rx = self.core().exit_watch();
            match rx.wait_for(Option::is_some).await {
                Ok(state) => (*state).flatten(),
                Err(_) => None,
            }
        })
    }

    fn info(&self) -> NodeInfo {
        let core = self.core();
        NodeInfo {
            id: core.id,
            name: core.name.clone(),
            pid: core.pid,
            pty: core.pty,
            command: core.command.clone(),
            state: if self.is_running() {
                NodeState::Running
            } else {
                NodeState::Exited
            },
            exit_code: self.exit_code(),
            cwd: None,
            title: None,
            foreground: None,
            started_ms: core.started_ms,
            last_output_ms: core.last_output_ms(),
            window: None,
            agent: None,
        }
    }
}
