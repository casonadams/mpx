//! display-popup: a command run on a pty of its own, drawn in a box over the panes with a screen of
//! its own. Keys go to it. It closes when the command exits (`-E`); without `-E` it stays up, and
//! the next key closes it.
use std::io::{self, Write};
use std::sync::Mutex;
use std::time::Duration;

use crossterm::cursor::MoveTo;
use crossterm::event::KeyEvent;
use crossterm::queue;
use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, PtySize, SlavePty, native_pty_system,
};
use tokio::sync::mpsc;

use super::app::Mode;
use super::canvas::write_sgr;
use super::overlay::widgets::{draw_box, inner};
use super::placement::{Extent, Offset};
use crate::keys::{KeyPolicy, encode_key};
use crate::layout::geometry::Rect;
use crate::vt::VtScreen;

/// `display-popup` options after parsing. The size defaults to 80% of the screen in each direction,
/// centred.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupOpts {
    pub width: Extent,
    pub height: Extent,
    pub x: Offset,
    pub y: Offset,
    /// `-E`: close as soon as the command exits.
    pub close_on_exit: bool,
    /// `-T`: text on the top border.
    pub title: Option<String>,
    /// `-d`, or the working directory of `target`; the TUI's own when neither is known.
    pub cwd: Option<String>,
    /// `-t`: the pane whose directory the popup starts in.
    pub target: Option<String>,
    /// Shell command the popup runs through the shell; the shell itself when `None`.
    pub command: Option<String>,
}

/// What the command on the popup's pty sends to the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupEvent {
    Output(Vec<u8>),
    /// The command ended, with its exit status when it has one.
    Exited(Option<i32>),
}

/// An open popup: its box, the screen its command draws, and the pty behind it.
pub struct PopupState {
    opts: PopupOpts,
    pub rect: Rect,
    pub screen: VtScreen,
    /// The command has ended and the popup waits for a key (no `-E`).
    pub exited: bool,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    rx: mpsc::UnboundedReceiver<PopupEvent>,
}

/// openpty(3) is not safe to call from several threads at once on macOS.
static OPEN_PTY: Mutex<()> = Mutex::new(());

/// The box a popup with `opts` is drawn in on a `screen` of the given size.
pub fn popup_rect(opts: &PopupOpts, screen: (u16, u16)) -> Rect {
    let width = opts.width.cells(screen.0).min(screen.0);
    let height = opts.height.cells(screen.1).min(screen.1);
    Rect::new(
        opts.x.start(screen.0, width),
        opts.y.start(screen.1, height),
        width,
        height,
    )
}

/// The size of the area inside a popup's box, which is what its command is given.
pub fn inner_size(rect: Rect) -> (u16, u16) {
    let inside = inner(rect);
    (inside.width.max(1), inside.height.max(1))
}

/// Resolves with the next event of the popup open in `mode`; never resolves otherwise.
pub async fn next_event(mode: &mut Mode) -> Option<PopupEvent> {
    let Mode::Popup(popup) = mode else {
        return std::future::pending().await;
    };
    popup.rx.recv().await
}

impl PopupState {
    /// Starts `opts`' command, through `shell` when it has one, on a pty sized for a `screen`.
    pub fn open(
        opts: PopupOpts,
        screen: (u16, u16),
        shell: &str,
        term: &str,
    ) -> Result<Self, String> {
        let rect = popup_rect(&opts, screen);
        let (cols, rows) = inner_size(rect);
        let pair = {
            let _one_at_a_time = OPEN_PTY.lock().unwrap_or_else(|e| e.into_inner());
            native_pty_system().openpty(pty_size(cols, rows))
        }
        .map_err(|e| format!("display-popup: failed to open a pty: {e}"))?;

        let child = pair
            .slave
            .spawn_command(popup_command(&opts, shell, term))
            .map_err(|e| format!("display-popup: failed to start {shell}: {e}"))?;
        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("display-popup: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("display-popup: {e}"))?;
        let rx = watch(child, reader, pair.slave);

        Ok(Self {
            opts,
            rect,
            screen: VtScreen::live(cols as usize, rows as usize),
            exited: false,
            master: pair.master,
            writer,
            killer,
            rx,
        })
    }

    pub fn title(&self) -> &str {
        self.opts.title.as_deref().unwrap_or("")
    }

    pub fn close_on_exit(&self) -> bool {
        self.opts.close_on_exit
    }

    /// Adopts the box `screen` now gives the popup, resizing its screen and pty when it changed.
    pub fn fit(&mut self, screen: (u16, u16)) {
        let rect = popup_rect(&self.opts, screen);
        if rect == self.rect {
            return;
        }
        self.rect = rect;
        let (cols, rows) = inner_size(rect);
        let grid = self.screen.grid();
        if (grid.cols, grid.rows) != (cols as usize, rows as usize) {
            self.screen.grid_mut().resize(cols as usize, rows as usize);
            let _ = self.master.resize(pty_size(cols, rows));
        }
    }

    /// Output of the command: drawn on the popup's screen, and any replies it asks for are sent back.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.screen.feed(bytes);
        for reply in self.screen.take_responses() {
            let _ = self.writer.write_all(&reply);
            let _ = self.writer.flush();
        }
    }

    /// Sends a key to the command, encoded with the input modes it set.
    pub fn send_key(&mut self, key: &KeyEvent, policy: KeyPolicy) -> io::Result<()> {
        let bytes = encode_key(key, &self.screen.modes(), policy);
        if bytes.is_empty() {
            return Ok(());
        }
        self.writer.write_all(&bytes)?;
        self.writer.flush()
    }

    /// Where the terminal cursor belongs when the popup has focus, if the command shows it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let cursor = self.screen.grid().cursor;
        cursor.visible.then_some((
            self.rect.x + 1 + cursor.x as u16,
            self.rect.y + 1 + cursor.y as u16,
        ))
    }

    /// Draws the box, its title and the command's screen.
    pub fn render<W: Write>(&self, out: &mut W) -> io::Result<()> {
        draw_box(out, self.rect, self.title(), "")?;
        let area = inner(self.rect);
        let grid = self.screen.grid();
        let mut style = None;
        for y in 0..area.height {
            queue!(out, MoveTo(area.x, area.y + y))?;
            for x in 0..area.width {
                let cell = grid
                    .get_cell(x as usize, y as usize)
                    .copied()
                    .unwrap_or_default();
                if style != Some(cell.style) {
                    write_sgr(out, &cell.style)?;
                    style = Some(cell.style);
                }
                let mut utf8 = [0u8; 4];
                out.write_all(cell.c.encode_utf8(&mut utf8).as_bytes())?;
            }
        }
        out.write_all(b"\x1b[0m")
    }
}

impl Drop for PopupState {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// The command a popup runs: `shell -c command`, or the shell alone, in the popup's directory.
fn popup_command(opts: &PopupOpts, shell: &str, term: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(shell);
    if let Some(line) = &opts.command {
        cmd.arg("-c");
        cmd.arg(line);
    }
    if let Some(dir) = &opts.cwd {
        cmd.cwd(dir);
    }
    cmd.env("TERM", term);
    cmd.env("COLORTERM", "truecolor");
    cmd
}

/// Reads the command's output on one thread and reports its exit on another, after the output's
/// end, so none of it is lost.
fn watch(
    mut child: Box<dyn Child + Send + Sync>,
    reader: Box<dyn io::Read + Send>,
    slave: Box<dyn SlavePty + Send>,
) -> mpsc::UnboundedReceiver<PopupEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
    let output = tx.clone();
    std::thread::spawn(move || {
        pump(reader, &output);
        let _ = eof_tx.send(());
    });
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code() as i32);
        drop(slave);
        let _ = eof_rx.recv_timeout(Duration::from_millis(500));
        let _ = tx.send(PopupEvent::Exited(code));
    });
    rx
}

/// Copies the command's output to the TUI until its end of file, or until the TUI has gone.
fn pump(mut reader: Box<dyn io::Read + Send>, out: &mpsc::UnboundedSender<PopupEvent>) {
    let _ = io::copy(&mut reader, &mut OutputSink(out));
}

/// A writer whose bytes reach the TUI as popup output; a closed channel is a write error.
struct OutputSink<'a>(&'a mpsc::UnboundedSender<PopupEvent>);

impl Write for OutputSink<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .send(PopupEvent::Output(buf.to_vec()))
            .map_err(|_| io::ErrorKind::BrokenPipe)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
