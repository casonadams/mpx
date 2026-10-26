use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};

use super::{Node, NodeError};
use crate::buffer::RingBuffer;
use crate::protocol::{NodeInfo, StreamEvent};
use crate::vt::VtScreen;

pub struct PtyNode {
    name: String,
    command: String,
    pid: Option<u32>,
    buffer: Arc<RwLock<RingBuffer>>,
    screen: Arc<RwLock<VtScreen>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    exit_code: Arc<RwLock<Option<i32>>>,
    exit_notify: Arc<Notify>,
    running: Arc<AtomicBool>,
}

impl PtyNode {
    pub fn spawn(
        name: String,
        cmd: String,
        args: &[String],
        cwd: Option<&str>,
        event_tx: broadcast::Sender<StreamEvent>,
    ) -> Result<Self, NodeError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| NodeError::Internal(format!("Failed to open PTY: {}", e)))?;

        let mut cmd_builder = CommandBuilder::new(&cmd);
        cmd_builder.args(args);
        if let Some(dir) = cwd {
            cmd_builder.cwd(dir);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd_builder)
            .map_err(|e| NodeError::Internal(format!("Failed to spawn command in PTY: {}", e)))?;
        let pid = child.process_id();

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| NodeError::Internal(format!("Failed to clone PTY reader: {}", e)))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| NodeError::Internal(format!("Failed to take PTY writer: {}", e)))?;

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

        // Background reader thread for synchronous PTY reader
        let buf_clone = Arc::clone(&buffer);
        let screen_clone = Arc::clone(&screen);
        let tx_clone = event_tx.clone();
        let node_name = name.clone();
        let running_read = Arc::clone(&running);

        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while running_read.load(Ordering::SeqCst) {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let bytes = &chunk[..n];
                        if let Ok(mut b) = buf_clone.try_write() {
                            b.write(bytes);
                        }
                        if let Ok(mut sc) = screen_clone.try_write() {
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

        // Background exit waiter thread
        let exit_clone = Arc::clone(&exit_code);
        let notify_clone = Arc::clone(&exit_notify);
        let running_clone = Arc::clone(&running);
        let tx_exit = event_tx.clone();
        let exit_name = name.clone();

        std::thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().map(|s| s.exit_code() as i32);
            if let Ok(mut ec) = exit_clone.try_write() {
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
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(pair.master)),
            exit_code,
            exit_notify,
            running,
        })
    }
}

impl Node for PtyNode {
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
            pty: true,
            command: self.command.clone(),
            status,
        }
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(NodeError::NotRunning);
        }
        let writer = Arc::clone(&self.writer);
        let data = data.to_vec();

        tokio::spawn(async move {
            let mut w = writer.lock().await;
            let _ = w.write_all(&data);
            let _ = w.flush();
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

    fn resize(&self, cols: u16, rows: u16) -> Result<(), NodeError> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if let Ok(master) = self.master.try_lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut screen) = self.screen.try_write() {
            screen.grid_mut().resize(cols as usize, rows as usize);
        }
        Ok(())
    }

    fn cursor(&self) -> (usize, usize, bool) {
        if let Ok(screen) = self.screen.try_read() {
            let c = screen.grid().cursor;
            (c.x, c.y, c.visible)
        } else {
            (0, 0, true)
        }
    }


    fn kill(&self) -> Result<(), NodeError> {
        self.running.store(false, Ordering::SeqCst);
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
