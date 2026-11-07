//! The pane actor: sole owner of a pane's process handle, output, screen, subscribers
//! and waiters. It never awaits, so it cannot block on a slow reader or on its session.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use super::pacing::Inflight;
use super::pane::{
    InputBuild, LogQuery, Notifier, PaneCtx, PaneMsg, PaneNotice, Reply, TermView, render_logs,
};
use super::process::{self, foreground_name as fg_name};
use super::pty::PtyTerminal;
use super::sink::Sink;
use super::sleep_until_opt;
use super::waits::{self, Cond, Observed};
use super::{Process, SpawnSpec, now_ms};
use crate::buffer::{PlainStream, RingBuffer, Utf8Stream, lossy};
use crate::protocol::{NodeInfo, NodeState, PaneId, StreamEvent, WaitOutcome};
use crate::vt::VtScreen;
use crate::vt::capture::{CaptureSpec, capture_text};
use crate::vt::keyframe::keyframe;

const PROBE_EVERY: Duration = Duration::from_millis(250);
const EXITED: &str = "Node has exited";

struct Waiter {
    cond: Cond,
    /// Text a `Match` searches: the screen at the request plus the plain output since.
    text: String,
    /// Length of `text` the last search covered.
    scanned: usize,
    plain: PlainStream,
    deadline: Option<Instant>,
    reply: Option<Reply<WaitOutcome>>,
}

impl Waiter {
    fn feed(&mut self, bytes: &[u8]) {
        if matches!(self.cond, Cond::Match(_)) {
            self.plain.feed(bytes, &mut self.text);
        }
    }

    fn outcome(&mut self, obs: &Observed) -> Option<WaitOutcome> {
        if matches!(self.cond, Cond::Match(_)) {
            if self.text.len() == self.scanned {
                return None;
            }
            self.scanned = self.text.len();
        }
        waits::satisfied(&self.cond, obs, &self.text)
    }
}

/// Retained output replayed into a screen with scrollback, and the offset it has consumed up to.
struct Replay {
    screen: VtScreen,
    fed: u64,
}

/// Facts about a pane's foreground process.
struct Probed {
    at: Instant,
    pid: Option<u32>,
    foreground: Option<String>,
    cwd: Option<String>,
    tty: Option<String>,
}

pub struct PaneActor {
    /// Output chunks its reader handed over and this actor has yet to process.
    inflight: Arc<Inflight>,
    name: String,
    id: PaneId,
    pty: bool,
    command: String,
    pid: Option<u32>,
    spawn_cwd: Option<String>,
    started_ms: u64,
    process: Option<Box<dyn Process>>,
    /// Terminal control while a PTY process runs; `None` for task panes and after exit.
    terminal: Option<PtyTerminal>,
    ring: RingBuffer,
    /// Scrollback lines a plain capture keeps (tmux `history-limit`).
    scrollback: usize,
    screen: VtScreen,
    utf8: Utf8Stream,
    /// Absolute offset up to which output has been decoded and emitted.
    emitted: u64,
    last_output_ms: u64,
    /// Size the retained output was last replayed from before a resize that changed the grid size.
    resized_from: Option<(usize, usize)>,
    /// Output offset where the last resize that changed the grid size happened: the retained output
    /// before it is replayed at `resized_from`, the output after it at the new size.
    resized_at: Option<u64>,
    /// The retained output replayed with scrollback; `None` until first needed and after a resize.
    replay: Option<Replay>,
    probed: Option<Probed>,
    /// `None` while running, `Some(code)` once exited.
    exit: Option<Option<i32>>,
    notify: Notifier,
    /// Whether session-wide subscribers exist; output is relayed to the session only then.
    relay: Arc<AtomicBool>,
    subs: Vec<Sink>,
    waiters: Vec<Waiter>,
    kill_waiters: Vec<Reply<()>>,
}

impl PaneActor {
    pub fn new(
        spec: &SpawnSpec,
        process: Box<dyn Process>,
        terminal: Option<PtyTerminal>,
        ctx: PaneCtx,
    ) -> Self {
        let pty = terminal.is_some();
        let command = if spec.args.is_empty() {
            spec.command.clone()
        } else {
            format!("{} {}", spec.command, spec.args.join(" "))
        };
        let started_ms = now_ms();
        Self {
            inflight: Arc::default(),
            name: spec.name.clone(),
            id: spec.id,
            pty,
            command,
            pid: process.pid(),
            spawn_cwd: spec.cwd.clone(),
            started_ms,
            process: Some(process),
            terminal,
            ring: RingBuffer::for_scrollback(spec.scrollback),
            scrollback: spec.scrollback,
            screen: VtScreen::live(80, 24),
            utf8: Utf8Stream::default(),
            emitted: 0,
            last_output_ms: started_ms,
            replay: None,
            resized_from: None,
            resized_at: None,
            probed: None,
            exit: None,
            notify: ctx.notify,
            relay: ctx.relay,
            subs: Vec::new(),
            waiters: Vec::new(),
            kill_waiters: Vec::new(),
        }
    }

    pub(super) async fn run(mut self, mut rx: UnboundedReceiver<PaneMsg>) {
        loop {
            let wake = self.next_wake();
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg),
                    None => break,
                },
                () = sleep_until_opt(wake) => self.on_timer(Instant::now()),
            }
        }
    }

    /// Counts processed output off `inflight`, which the pane's reader waits on.
    pub fn with_inflight(mut self, inflight: Arc<Inflight>) -> Self {
        self.inflight = inflight;
        self
    }

    pub fn handle(&mut self, msg: PaneMsg) {
        match msg {
            PaneMsg::Output(bytes) => {
                self.on_output(&bytes);
                self.inflight.done();
            }
            PaneMsg::Exited(code) => self.on_exit(code),
            PaneMsg::Info { full, reply } => {
                let _ = reply.send(self.info(full));
            }
            PaneMsg::Label { reply } => {
                let _ = reply.send(self.label());
            }
            PaneMsg::Logs { query, reply } => self.logs(query, reply),
            PaneMsg::Screen { reply } => {
                let _ = reply.send(self.screen.snapshot());
            }
            PaneMsg::Keyframe { reply } => {
                let grid = self.screen.grid();
                let size = (grid.cols as u16, grid.rows as u16);
                let _ = reply.send((keyframe(grid), self.ring.total_written(), size));
            }
            PaneMsg::Capture { spec, reply } => {
                let _ = reply.send(self.capture(&spec));
            }
            PaneMsg::Cwd { reply } => {
                let _ = reply.send(self.cwd());
            }
            PaneMsg::Input { build, reply } => {
                let _ = reply.send(self.input(build));
            }
            PaneMsg::Resize { cols, rows, reply } => {
                let _ = reply.send(self.resize(cols, rows));
            }
            PaneMsg::Kill { reply } => self.kill(reply),
            PaneMsg::Wait {
                cond,
                timeout,
                reply,
            } => self.add_waiter(cond, timeout, reply),
            PaneMsg::Subscribe { since, sink, reply } => {
                self.subscribe(since, sink);
                let _ = reply.send(());
            }
        }
    }

    fn on_output(&mut self, bytes: &[u8]) {
        self.screen.feed(bytes);
        for reply in self.screen.take_responses() {
            self.write_raw(&reply);
        }
        let _ = self.screen.take_passthroughs();
        self.ring.write(bytes);
        self.last_output_ms = now_ms();
        self.emit_text(bytes);
        for waiter in &mut self.waiters {
            waiter.feed(bytes);
        }
        self.check_waiters();
    }

    /// Decodes the chunk and emits the text it completes, covering the byte range it advanced over.
    fn emit_text(&mut self, bytes: &[u8]) {
        let (data, held) = self.utf8.feed(bytes);
        let offset = self.emitted;
        let next = self.ring.total_written() - held as u64;
        self.emitted = next;
        if !data.is_empty() && self.has_consumers() {
            let name = self.name.clone();
            self.emit(StreamEvent::Output {
                name,
                data,
                offset,
                next,
            });
        }
    }

    fn on_exit(&mut self, code: Option<i32>) {
        if self.exit.is_some() {
            return;
        }
        self.exit = Some(code);
        self.process = None;
        self.terminal = None;
        self.probed = None;
        let exit = StreamEvent::Exit {
            name: self.name.clone(),
            code,
        };
        self.subs.retain_mut(|sink| sink.push(exit.clone()));
        // Session first: the exit must be processed before any waiter for it is answered.
        (self.notify)(PaneNotice::Exited {
            name: self.name.clone(),
            id: self.id,
            code,
        });
        for reply in std::mem::take(&mut self.kill_waiters) {
            let _ = reply.send(());
        }
        self.check_waiters();
    }

    fn kill(&mut self, reply: Reply<()>) {
        if self.exit.is_some() {
            let _ = reply.send(());
            return;
        }
        if let Some(process) = &self.process {
            let _ = process.kill();
        }
        self.kill_waiters.push(reply);
    }

    fn write_raw(&self, data: &[u8]) {
        if let Some(process) = &self.process {
            let _ = process.send_input(data);
        }
    }

    fn input(&mut self, build: InputBuild) -> Result<(), String> {
        let process = self.process.as_ref().ok_or_else(|| EXITED.to_string())?;
        let view = TermView {
            pty: self.pty,
            modes: self.screen.modes(),
        };
        let bytes = build(&view);
        process.send_input(&bytes).map_err(|e| e.to_string())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let grid = self.screen.grid();
        let from = (grid.cols, grid.rows);
        let to = (cols as usize, rows as usize);
        if from != to {
            // Rebuilt from the retained output, as the app's screens rebuild it, so the rows a grow
            // pulls back from scrollback are the same at every end. The output after this point is
            // fed to the new grid as it arrives.
            let rebuilt = VtScreen::resized_grid(&self.ring.snapshot(), &[], from, to, 0);
            *self.screen.grid_mut() = rebuilt;
            self.resized_from = Some(from);
            self.resized_at = Some(self.ring.total_written());
        }
        self.replay = None;
        if let Some(terminal) = &self.terminal {
            terminal.resize(cols, rows);
        }
        Ok(())
    }

    fn subscribe(&mut self, since: Option<u64>, mut sink: Sink) {
        if let Some(since) = since {
            let (bytes, next) = self.ring.since(since);
            if !bytes.is_empty() {
                sink.push(StreamEvent::Output {
                    name: self.name.clone(),
                    data: lossy(bytes),
                    offset: since,
                    next,
                });
            }
        }
        if let Some(code) = self.exit {
            sink.push(StreamEvent::Exit {
                name: self.name.clone(),
                code,
            });
        }
        self.subs.push(sink);
    }

    fn logs(&self, query: LogQuery, reply: Reply<(String, u64)>) {
        let screen = self.pty.then_some(&self.screen);
        let _ = reply.send(render_logs(&self.ring, screen, &query, self.scrollback));
    }

    /// The history and screen `capture-pane` reads from.
    fn capture(&mut self, spec: &CaptureSpec) -> String {
        capture_text(self.replay().grid(), spec)
    }

    fn info(&mut self, full: bool) -> NodeInfo {
        let (state, exit_code) = match self.exit {
            Some(code) => (NodeState::Exited, code),
            None => (NodeState::Running, None),
        };
        let (foreground, cwd, foreground_pid, tty) = if full {
            let probed = self.probed();
            (
                probed.foreground.clone(),
                probed.cwd.clone(),
                probed.pid,
                probed.tty.clone(),
            )
        } else {
            (None, None, None, None)
        };
        let modes = self.pty.then(|| self.screen.modes());
        let size = self.pty.then(|| {
            (
                self.screen.grid().cols as u16,
                self.screen.grid().rows as u16,
            )
        });
        let cursor = self.pty.then(|| {
            let at = self.screen.grid().cursor;
            (at.x as u16, at.y as u16)
        });
        let history_size = self
            .pty
            .then(|| self.screen.grid().rows_scrolled_off().min(self.scrollback));
        let resized_from = self
            .resized_from
            .map(|(cols, rows)| (cols as u16, rows as u16));
        NodeInfo {
            name: self.name.clone(),
            id: self.id,
            pid: self.pid,
            pty: self.pty,
            command: self.command.clone(),
            state,
            exit_code,
            cwd,
            title: self.screen.title().map(str::to_string),
            foreground,
            foreground_pid,
            tty,
            started_ms: self.started_ms,
            last_output_ms: self.last_output_ms,
            window: None,
            modes,
            cursor,
            history_size,
            size,
            resized_from,
            input_off: None,
            marked: None,
            resized_at: self.resized_at,
        }
    }

    /// The retained output replayed with scrollback at the pane's size, brought up to date by
    /// feeding it only the output since its last use. A resize drops it.
    fn replay(&mut self) -> &VtScreen {
        let replay = self.replay.get_or_insert_with(|| {
            let grid = self.screen.grid();
            let mut screen = VtScreen::new(grid.cols, grid.rows);
            screen.grid_mut().set_history_limit(self.scrollback);
            Replay {
                screen,
                fed: self.ring.oldest_offset(),
            }
        });
        let (bytes, next) = self.ring.since(replay.fed);
        replay.screen.feed(&bytes);
        let _ = replay.screen.take_responses();
        let _ = replay.screen.take_passthroughs();
        let _ = replay.screen.take_alerts();
        replay.fed = next;
        &replay.screen
    }

    fn cwd(&self) -> Option<String> {
        self.fg_pid()
            .and_then(process::cwd_of)
            .or_else(|| self.spawn_cwd.clone())
    }

    fn label(&mut self) -> Option<String> {
        self.probed().foreground.clone()
    }

    fn add_waiter(&mut self, cond: Cond, timeout: Option<Duration>, reply: Reply<WaitOutcome>) {
        let text = match cond {
            Cond::Match(_) => format!("{}\n", self.screen.snapshot()),
            _ => String::new(),
        };
        let deadline = timeout.map(|d| Instant::now() + d);
        let mut waiter = Waiter {
            cond,
            text,
            scanned: 0,
            plain: PlainStream::default(),
            deadline,
            reply: Some(reply),
        };
        let obs = self.observed();
        if let Some(outcome) = waiter.outcome(&obs) {
            if let Some(reply) = waiter.reply {
                let _ = reply.send(outcome);
            }
            return;
        }
        self.waiters.push(waiter);
    }

    fn check_waiters(&mut self) {
        self.settle_waiters(Waiter::outcome);
    }

    fn expire_waiters(&mut self, now: Instant) {
        self.settle_waiters(|waiter, obs| {
            waiter
                .deadline
                .is_some_and(|d| now >= d)
                .then(|| waits::timed_out(obs))
        });
    }

    /// Answers and drops each waiter for which `outcome` yields a result.
    fn settle_waiters(
        &mut self,
        mut outcome: impl FnMut(&mut Waiter, &Observed) -> Option<WaitOutcome>,
    ) {
        if self.waiters.is_empty() {
            return;
        }
        let obs = self.observed();
        self.waiters
            .retain_mut(|waiter| match outcome(waiter, &obs) {
                Some(outcome) => {
                    if let Some(reply) = waiter.reply.take() {
                        let _ = reply.send(outcome);
                    }
                    false
                }
                None => true,
            });
    }

    fn observed(&self) -> Observed {
        Observed {
            now_ms: now_ms(),
            last_output_ms: self.last_output_ms,
            exit: self.exit,
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        let mut due = self.waiters.iter().filter_map(|w| w.deadline).min();
        for w in &self.waiters {
            if let Some(t_ms) = w.cond.idle_due(self.last_output_ms) {
                due = soonest(due, Some(instant_at(t_ms)));
            }
        }
        due
    }

    pub fn on_timer(&mut self, now: Instant) {
        self.expire_waiters(now);
        self.check_waiters();
    }

    /// The foreground process's facts, looked up at most once per `PROBE_EVERY`: each lookup is
    /// several system calls made on the thread that also processes the pane's output.
    fn probed(&mut self) -> &Probed {
        match self.probed.take() {
            Some(probed) if probed.at.elapsed() < PROBE_EVERY => self.probed.insert(probed),
            _ => {
                let probed = self.lookup();
                self.probed.insert(probed)
            }
        }
    }

    fn lookup(&self) -> Probed {
        let pid = self.fg_pid();
        Probed {
            at: Instant::now(),
            pid,
            foreground: pid.map(process::identity).and_then(|id| fg_name(&id)),
            cwd: pid
                .and_then(process::cwd_of)
                .or_else(|| self.spawn_cwd.clone()),
            tty: self.terminal.as_ref().and_then(|t| t.tty_name()),
        }
    }

    fn fg_pid(&self) -> Option<u32> {
        self.process.as_ref()?;
        self.terminal
            .as_ref()
            .and_then(|t| t.foreground_pid())
            .or(self.pid)
    }

    fn emit(&mut self, event: StreamEvent) {
        let relay = self.relay.load(Ordering::Relaxed);
        let last = self.subs.len().checked_sub(1);
        let mut pending = Some(event);
        let mut index = 0;
        self.subs.retain_mut(|sink| {
            let owned = if !relay && Some(index) == last {
                pending.take()
            } else {
                pending.clone()
            };
            index += 1;
            owned.is_some_and(|event| sink.push(event))
        });
        if let Some(event) = pending.filter(|_| relay) {
            (self.notify)(PaneNotice::Event(event));
        }
    }

    fn has_consumers(&self) -> bool {
        !self.subs.is_empty() || self.relay.load(Ordering::Relaxed)
    }
}

fn soonest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (other, None) | (None, other) => other,
    }
}

fn instant_at(target_ms: u64) -> Instant {
    let now_ms = now_ms();
    let delta = Duration::from_millis(target_ms.saturating_sub(now_ms));
    Instant::now() + delta
}
