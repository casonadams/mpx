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
use crate::buffer::{PlainStream, RING_CAPACITY, RingBuffer, Utf8Stream, lossy};
use crate::protocol::{NodeInfo, NodeState, PaneId, StreamEvent, WaitOutcome};
use crate::vt::VtScreen;
use crate::vt::capture::{CaptureSpec, capture_pane_text};
use crate::vt::keyframe::keyframe;

/// Saving rewrites the whole saved tail, so output must grow it by this fraction (1 in N) before a
/// save is due: a pane that prints steadily is not rewritten in full on every flush.
const SAVE_AFTER_GROWTH: u64 = 8;
/// How long output of any size may wait to be saved.
const SAVE_AFTER_MS: u64 = 30_000;

/// Whether `new_bytes` of output written `age_ms` ago since the last save are due to be saved,
/// when `saved_tail` bytes are what a save writes.
pub fn scrollback_save_due(new_bytes: u64, saved_tail: u64, age_ms: u64) -> bool {
    new_bytes >= saved_tail / SAVE_AFTER_GROWTH || age_ms >= SAVE_AFTER_MS
}

const PROBE_EVERY: Duration = Duration::from_millis(250);
const EXITED: &str = "Node has exited";
/// Most recent bytes of text a `Match` waiter searches, at least. The text is trimmed back to this
/// once it reaches twice as much, so the trimming moves memory once per this much output.
const MAX_MATCH_TEXT: usize = 256 * 1024;

struct Waiter {
    cond: Cond,
    /// Text a `Match` searches: the screen at the request plus the plain output since.
    text: String,
    /// Whether `text` changed since the last search.
    unsearched: bool,
    plain: PlainStream,
    deadline: Option<Instant>,
    reply: Option<Reply<WaitOutcome>>,
}

impl Waiter {
    fn feed(&mut self, bytes: &[u8]) {
        if matches!(self.cond, Cond::Match(_)) {
            self.plain.feed(bytes, &mut self.text);
            self.unsearched = true;
            if self.text.len() >= 2 * MAX_MATCH_TEXT {
                let len = self.text.len();
                let cut = (len - MAX_MATCH_TEXT..=len)
                    .find(|&i| self.text.is_char_boundary(i))
                    .unwrap_or(len);
                self.text.drain(..cut);
            }
        }
    }

    fn outcome(&mut self, obs: &Observed) -> Option<WaitOutcome> {
        if matches!(self.cond, Cond::Match(_)) {
            if !self.unsearched {
                return None;
            }
            self.unsearched = false;
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
    last_output_ms: u64,
    /// Size the retained output was last replayed from before a resize that changed the grid size.
    resized_from: Option<(usize, usize)>,
    /// Output offset where the last resize that changed the grid size happened: the retained output
    /// before it is replayed at `resized_from`, the output after it at the new size.
    resized_at: Option<u64>,
    /// The retained output replayed with scrollback; `None` until first needed and after a resize.
    replay: Option<Replay>,
    /// Output offset up to which the retained output was last handed out to be saved.
    saved_at: u64,
    /// When that happened.
    saved_ms: u64,
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
        let mut ring = RingBuffer::for_scrollback(spec.scrollback);
        let mut screen = VtScreen::live(spec.size.0.max(1).into(), spec.size.1.max(1).into());
        ring.write(&spec.restored);
        screen.feed(&spec.restored);
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
            ring,
            scrollback: spec.scrollback,
            screen,
            utf8: Utf8Stream::default(),
            last_output_ms: started_ms,
            replay: None,
            saved_at: 0,
            saved_ms: started_ms,
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
            PaneMsg::UnsavedScrollback { force, reply } => {
                let _ = reply.send(self.unsaved_scrollback(force));
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
        let _ = self.screen.take_alerts();
        self.last_output_ms = now_ms();
        self.emit_text(bytes);
        for waiter in &mut self.waiters {
            waiter.feed(bytes);
        }
        self.check_waiters();
    }

    /// Decodes the chunk, logs the bytes of the characters it completes and emits their text. A
    /// character split across chunks is logged with the chunk that completes it, so every offset
    /// the log reports (reads, keyframes, resizes) falls between characters, as events' do.
    fn emit_text(&mut self, bytes: &[u8]) {
        let (data, complete) = self.utf8.decode(bytes);
        let offset = self.ring.total_written();
        self.ring.write(&complete);
        let next = self.ring.total_written();
        if !data.is_empty() && self.has_consumers() {
            let name = self.name.clone();
            self.emit(StreamEvent::Output {
                name,
                data: data.into(),
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
        crate::clients::check_size(cols, rows)?;
        let grid = self.screen.grid();
        let from = (grid.cols, grid.rows);
        let to = (cols as usize, rows as usize);
        if from != to {
            if !self.screen.resize_in_place(to) {
                // Rebuilt from the newest `RING_CAPACITY` bytes of the retained output: the lines
                // reflow, or a grow pulls rows back from scrollback the live screen does not keep.
                // A longer log (kept for scrollback) would make every resize replay all of it.
                let log = self.ring.tail(RING_CAPACITY);
                *self.screen.grid_mut() = VtScreen::resized_grid(&log, &[], from, to, 0);
            }
            self.resized_from = Some(from);
            self.resized_at = Some(self.ring.total_written());
            self.replay = None;
        }
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
                    data: lossy(bytes).into(),
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

    fn logs(&mut self, query: LogQuery, reply: Reply<(String, u64)>) {
        if self.pty && query.plain && query.since.is_none() && query.until.is_none() {
            let (history, live) = self.replay();
            let text = history.scrollback_text_over(live.grid(), query.tail);
            let _ = reply.send((text, self.ring.total_written()));
            return;
        }
        let screen = self.pty.then_some(&self.screen);
        let _ = reply.send(render_logs(&self.ring, screen, &query, self.scrollback));
    }

    /// The history and screen `capture-pane` reads from.
    fn capture(&mut self, spec: &CaptureSpec) -> String {
        let (history, live) = self.replay();
        capture_pane_text(history.grid(), live.grid(), spec)
    }

    /// The newest `RING_CAPACITY` bytes of output, from a line start, with the screen size, when
    /// output arrived since they were last handed out.
    fn unsaved_scrollback(&mut self, force: bool) -> Option<(Vec<u8>, (u16, u16))> {
        let written = self.ring.total_written();
        let (new_bytes, age_ms) = (
            written - self.saved_at,
            now_ms().saturating_sub(self.saved_ms),
        );
        let saved_tail = self.ring.len().min(RING_CAPACITY) as u64;
        if new_bytes == 0 || !(force || scrollback_save_due(new_bytes, saved_tail, age_ms)) {
            return None;
        }
        self.saved_at = written;
        self.saved_ms = now_ms();
        let grid = self.screen.grid();
        let size = (grid.cols as u16, grid.rows as u16);
        Some((self.ring.line_tail(RING_CAPACITY), size))
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
    /// feeding it only the output since its last use, and the live screen. The replay supplies the
    /// history; the visible rows are the live screen's, which took each resize where it happened.
    /// A resize drops the replay.
    fn replay(&mut self) -> (&VtScreen, &VtScreen) {
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
        (&replay.screen, &self.screen)
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
            unsearched: true,
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
