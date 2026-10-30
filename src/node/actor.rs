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
use super::waits::{self, Cond, Observed};
use super::{Process, SpawnSpec, now_ms};
use super::sleep_until_opt;
use crate::buffer::{RingBuffer, Utf8Stream, strip_ansi};
use crate::protocol::{NodeInfo, NodeState, PaneId, StreamEvent, WaitOutcome};
use crate::vt::VtScreen;

const PROBE_EVERY: Duration = Duration::from_millis(250);
const EXITED: &str = "Node has exited";

struct Waiter {
    cond: Cond,
    /// Output offset when the wait began (match only).
    from: u64,
    /// Screen text when the wait began (match only).
    screen: String,
    deadline: Option<Instant>,
    reply: Option<Reply<WaitOutcome>>,
}

impl Waiter {
    fn outcome(&self, obs: &Observed, ring: &RingBuffer) -> Option<WaitOutcome> {
        let text = || format!("{}\n{}", self.screen, strip_ansi(&ring.since(self.from).0));
        waits::satisfied(&self.cond, obs, &text)
    }
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
    screen: VtScreen,
    utf8: Utf8Stream,
    /// Absolute offset up to which output has been decoded and emitted.
    emitted: u64,
    last_output_ms: u64,
    next_probe: Instant,
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
            ring: RingBuffer::default(),
            screen: VtScreen::live(80, 24),
            utf8: Utf8Stream::default(),
            emitted: 0,
            last_output_ms: started_ms,
            next_probe: Instant::now(),
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
                self.refresh();
                let _ = reply.send(self.info(full));
            }
            PaneMsg::Label { reply } => {
                self.refresh();
                let _ = reply.send(self.label());
            }
            PaneMsg::Logs { query, reply } => self.logs(query, reply),
            PaneMsg::Screen { reply } => {
                let _ = reply.send(self.screen.snapshot());
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
        if Instant::now() >= self.next_probe {
            self.probe();
        }
        self.check_waiters();
    }

    /// Decodes the chunk and emits the text it completes, covering the byte range it advanced over.
    fn emit_text(&mut self, bytes: &[u8]) {
        let (data, held) = self.utf8.feed(bytes);
        let offset = self.emitted;
        let next = self.ring.total_written() - held as u64;
        self.emitted = next;
        if !data.is_empty() {
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
        self.screen.grid_mut().resize(cols as usize, rows as usize);
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
                    data: String::from_utf8_lossy(&bytes).into_owned(),
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
        let _ = reply.send(render_logs(&self.ring, screen, &query));
    }

    fn info(&self, full: bool) -> NodeInfo {
        let (state, exit_code) = match self.exit {
            Some(code) => (NodeState::Exited, code),
            None => (NodeState::Running, None),
        };
        let (foreground, cwd) = if full {
            (self.foreground(), self.cwd())
        } else {
            (None, None)
        };
        let modes = self.pty.then(|| self.screen.modes());
        let size = self
            .pty
            .then(|| (self.screen.grid().cols as u16, self.screen.grid().rows as u16));
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
            started_ms: self.started_ms,
            last_output_ms: self.last_output_ms,
            window: None,
            modes,
            size,
            marked: None,
        }
    }
    fn cwd(&self) -> Option<String> {
        self.fg_pid()
            .and_then(process::cwd_of)
            .or_else(|| self.spawn_cwd.clone())
    }

    fn label(&self) -> Option<String> {
        self.foreground()
    }

    fn foreground(&self) -> Option<String> {
        self.fg_pid()
            .map(process::identity)
            .and_then(|id| fg_name(&id))
    }

    fn add_waiter(&mut self, cond: Cond, timeout: Option<Duration>, reply: Reply<WaitOutcome>) {
        let from = self.ring.total_written();
        let screen = self.screen.snapshot();
        let deadline = timeout.map(|d| Instant::now() + d);
        let waiter = Waiter {
            cond,
            from,
            screen,
            deadline,
            reply: Some(reply),
        };
        let obs = self.observed();
        if let Some(outcome) = waiter.outcome(&obs, &self.ring) {
            if let Some(reply) = waiter.reply {
                let _ = reply.send(outcome);
            }
            return;
        }
        self.waiters.push(waiter);
    }

    fn check_waiters(&mut self) {
        let obs = self.observed();
        let ring = &self.ring;
        self.waiters.retain_mut(|waiter| {
            if let Some(outcome) = waiter.outcome(&obs, ring) {
                if let Some(reply) = waiter.reply.take() {
                    let _ = reply.send(outcome);
                }
                false
            } else {
                true
            }
        });
    }

    fn expire_waiters(&mut self, now: Instant) {
        let obs = self.observed();
        self.waiters.retain_mut(|waiter| {
            if waiter.deadline.is_some_and(|d| now >= d) {
                if let Some(reply) = waiter.reply.take() {
                    let _ = reply.send(waits::timed_out(&obs));
                }
                false
            } else {
                true
            }
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
        let mut due = self
            .waiters
            .iter()
            .filter_map(|w| w.deadline)
            .min();
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

    fn refresh(&mut self) {
        self.probe();
    }

    fn probe(&mut self) {
        self.next_probe = Instant::now() + PROBE_EVERY;
    }

    fn fg_pid(&self) -> Option<u32> {
        self.process.as_ref()?;
        self.terminal
            .as_ref()
            .and_then(|t| t.foreground_pid())
            .or(self.pid)
    }


    fn emit(&mut self, event: StreamEvent) {
        self.subs.retain_mut(|sink| sink.push(event.clone()));
        if self.relay.load(Ordering::Relaxed) {
            (self.notify)(PaneNotice::Event(event));
        }
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
