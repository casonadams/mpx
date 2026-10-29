//! The pane actor: sole owner of a pane's process handle, output, screen, agent state, subscribers
//! and waiters. It never awaits, so it cannot block on a slow reader or on its session.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use super::agent::AgentTracker;
use super::pacing::Inflight;
use super::pane::{
    InputBuild, LogQuery, Notifier, PaneCtx, PaneMsg, PaneNotice, Reply, TermView, render_logs,
    replay_text,
};
use super::process::{self, detection_names, foreground_name as fg_name};
use super::pty::PtyTerminal;
use super::sink::Sink;
use super::waits::{self, Cond, Observed};
use super::{Process, SpawnSpec, now_ms};
use super::sleep_until_opt;
use crate::agents::{self, Harness};
use crate::buffer::{RingBuffer, Utf8Stream, strip_ansi};
use crate::protocol::{NodeInfo, NodeState, PaneId, StreamEvent, WaitOutcome};
use crate::vt::{Alert, VtScreen};

const PROBE_EVERY: Duration = Duration::from_millis(250);
const EXITED: &str = "Node has exited";

struct Waiter {
    cond: Cond,
    /// Output offset when the wait began (match only).
    from: u64,
    /// Screen text when the wait began (match only).
    screen: String,
    deadline: Option<Instant>,
    reply: Reply<WaitOutcome>,
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
    agent: AgentTracker,
    harnesses: Arc<Vec<Harness>>,
    idle_ms: u64,
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
            agent: AgentTracker::default(),
            harnesses: ctx.harnesses,
            idle_ms: ctx.idle_ms,
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
            PaneMsg::Report { report, reply } => {
                self.agent_mut(|a, now| a.report(&report, now));
                self.check_waiters();
                let _ = reply.send(());
            }
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
        self.ring.write(bytes);
        self.last_output_ms = now_ms();
        self.emit_text(bytes);
        let alerts = self.screen.take_alerts();
        self.apply_alerts(alerts);
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
        self.agent_mut(|a, now| a.exited(now));
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

    fn input(&mut self, build: InputBuild) -> Result<(), String> {
        if self.exit.is_some() {
            return Err(EXITED.to_string());
        }
        let view = TermView {
            pty: self.pty,
            modes: self.screen.modes(),
        };
        let bytes = build(&view);
        let process = self.process.as_ref().ok_or_else(|| EXITED.to_string())?;
        process.send_input(&bytes).map_err(|e| e.to_string())?;
        self.agent_mut(|a, now| a.input(now));
        self.check_waiters();
        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if let Some(terminal) = &self.terminal {
            terminal.resize(cols, rows);
        }
        if self.pty {
            self.screen
                .grid_mut()
                .resize(usize::from(cols), usize::from(rows));
        }
        Ok(())
    }

    fn add_waiter(&mut self, cond: Cond, timeout: Option<Duration>, reply: Reply<WaitOutcome>) {
        if cond.watches_state() {
            self.probe();
        }
        self.waiters.push(Waiter {
            from: self.ring.total_written(),
            screen: self.screen.snapshot(),
            deadline: timeout.map(|d| Instant::now() + d),
            cond,
            reply,
        });
        self.check_waiters();
    }

    fn subscribe(&mut self, since: Option<u64>, mut sink: Sink) {
        if let Some(since) = since {
            self.replay(since, &mut sink);
        }
        self.subs.push(sink);
    }

    /// Queues output from `since` up to the live frontier, so the subscriber continues without a gap.
    fn replay(&self, since: u64, sink: &mut Sink) {
        let end = self.emitted;
        let start = since.max(self.ring.oldest_offset()).min(end);
        if start >= end {
            return;
        }
        let (mut bytes, _) = self.ring.since(start);
        bytes.truncate(usize::try_from(end - start).unwrap_or(usize::MAX));
        let (data, _) = Utf8Stream::default().feed(&bytes);
        if !data.is_empty() {
            sink.push(StreamEvent::Output {
                name: self.name.clone(),
                data,
                offset: start,
                next: end,
            });
        }
    }

    fn emit(&mut self, event: StreamEvent) {
        self.subs.retain_mut(|sink| sink.push(event.clone()));
        if self.relay.load(Ordering::Relaxed) {
            (self.notify)(PaneNotice::Event(event));
        }
    }

    fn check_waiters(&mut self) {
        let obs = self.observed();
        for waiter in std::mem::take(&mut self.waiters) {
            match waiter.outcome(&obs, &self.ring) {
                Some(outcome) => {
                    let _ = waiter.reply.send(outcome);
                }
                None => self.waiters.push(waiter),
            }
        }
    }

    fn expire_waiters(&mut self, now: Instant) {
        let obs = self.observed();
        for waiter in std::mem::take(&mut self.waiters) {
            if waiter.deadline.is_some_and(|d| d <= now) {
                let _ = waiter.reply.send(waits::timed_out(&obs));
            } else {
                self.waiters.push(waiter);
            }
        }
    }

    fn observed(&self) -> Observed {
        Observed {
            now_ms: now_ms(),
            last_output_ms: self.last_output_ms,
            exit: self.exit,
            state: self.agent.state(),
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        let mut due = None;
        for waiter in &self.waiters {
            due = soonest(due, waiter.deadline);
            due = soonest(
                due,
                waiter.cond.idle_due(self.last_output_ms).map(instant_at),
            );
            if waiter.cond.watches_state() {
                due = soonest(due, Some(self.next_probe));
            }
        }
        if self.agent.ticks() {
            due = soonest(due, Some(instant_at(self.last_output_ms + self.idle_ms)));
        }
        due
    }

    pub fn on_timer(&mut self, now: Instant) {
        self.expire_waiters(now);
        if self.agent.ticks() {
            self.tick_agent();
        }
        if now >= self.next_probe && self.waiters.iter().any(|w| w.cond.watches_state()) {
            self.probe();
        }
        self.check_waiters();
    }

    /// Applies pending alerts and re-detects the foreground agent.
    fn refresh(&mut self) {
        let alerts = self.screen.take_alerts();
        self.apply_alerts(alerts);
        self.probe();
    }

    fn probe(&mut self) {
        self.next_probe = Instant::now() + PROBE_EVERY;
        if self.exit.is_some() {
            return;
        }
        let harness = self
            .foreground_names()
            .iter()
            .find_map(|n| agents::detect(&self.harnesses, n))
            .map(|h| h.name.clone());
        self.agent_mut(|a, now| a.observe_foreground(harness.as_deref(), now));
        self.tick_agent();
    }

    fn tick_agent(&mut self) {
        let (last, idle) = (self.last_output_ms, self.idle_ms);
        self.agent_mut(|a, now| a.tick(now, last, idle));
        self.check_waiters();
    }

    fn apply_alerts(&mut self, alerts: Vec<Alert>) {
        for alert in alerts {
            match alert {
                Alert::Bell => self.agent_mut(|a, now| a.alert(None, now)),
                Alert::Notify(msg) => self.agent_mut(|a, now| a.alert(Some(msg), now)),
            }
        }
    }

    /// Runs a change on the agent tracker and announces the new state when it differs.
    fn agent_mut<T>(&mut self, change: impl FnOnce(&mut AgentTracker, u64) -> T) -> T {
        let before = self.agent.info();
        let out = change(&mut self.agent, now_ms());
        let after = self.agent.info();
        if before != after {
            let name = self.name.clone();
            self.emit(StreamEvent::Agent { name, agent: after });
        }
        out
    }

    fn fg_pid(&self) -> Option<u32> {
        self.process.as_ref()?;
        self.terminal
            .as_ref()
            .and_then(|t| t.foreground_pid())
            .or(self.pid)
    }

    fn foreground_names(&self) -> Vec<String> {
        self.fg_pid()
            .map(process::identity)
            .map(|id| detection_names(&id))
            .unwrap_or_default()
    }

    fn foreground_name(&self) -> Option<String> {
        self.terminal
            .as_ref()
            .and_then(|t| t.foreground_pid())
            .map(process::identity)
            .and_then(|id| fg_name(&id))
    }

    fn label(&self) -> Option<String> {
        self.agent
            .info()
            .map(|a| a.harness)
            .or_else(|| self.foreground_name())
    }

    fn cwd(&self) -> Option<String> {
        self.fg_pid()
            .and_then(process::cwd_of)
            .or_else(|| self.spawn_cwd.clone())
    }

    fn info(&self, full: bool) -> NodeInfo {
        let mut info = NodeInfo::new(self.name.clone(), self.pty, self.command.clone());
        info.id = self.id;
        info.pid = self.pid;
        info.state = if self.exit.is_none() {
            NodeState::Running
        } else {
            NodeState::Exited
        };
        info.exit_code = self.exit.flatten();
        info.started_ms = self.started_ms;
        info.last_output_ms = self.last_output_ms;
        info.agent = self.agent.info();
        info.modes = self.pty.then(|| self.screen.modes());
        if full {
            info.title = self.screen.title().map(str::to_string);
            info.foreground = self.foreground_name();
            info.cwd = self.cwd();
        }
        info
    }

    /// A plain read of a terminal's whole history replays the ring, milliseconds for a full one;
    /// that runs off the runtime, whose one thread serves every pane.
    fn logs(&self, query: LogQuery, reply: Reply<(String, u64)>) {
        if !(self.pty && query.plain && query.since.is_none()) {
            let _ = reply.send(render_logs(
                &self.ring,
                self.pty.then_some(&self.screen),
                &query,
            ));
            return;
        }
        let (output, next) = (self.ring.snapshot(), self.ring.total_written());
        let size = (self.screen.grid().cols, self.screen.grid().rows);
        tokio::task::spawn_blocking(move || {
            let _ = reply.send((replay_text(&output, size, query.tail), next));
        });
    }

    fn write_raw(&self, bytes: &[u8]) {
        if let Some(process) = &self.process {
            let _ = process.send_input(bytes);
        }
    }
}

fn soonest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) | (None, x) => x,
    }
}

/// The instant at which the wall clock reaches `target_ms`.
fn instant_at(target_ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(target_ms.saturating_sub(now_ms()))
}

