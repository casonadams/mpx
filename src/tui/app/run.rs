use crossterm::event::Event;
use std::io::{BufWriter, Write, stdout};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::input::latest_drag;
use super::pacing::needs_redraw;
use super::state::{App, AppOptions, Flow, StartView, StreamRx};
use crate::config::ExtendedKeys;
use crate::node::sleep_until_opt;
use crate::protocol::StreamEvent;
use crate::tui::frame;
use crate::tui::job::Job;
use crate::tui::popup::{PopupEvent, next_event};
use crate::tui::render::HostColors;
use crate::tui::statusline;
use crate::tui::terminal::{TerminalGuard, query_terminal};
use crate::tui::warm::Loaded;

/// Runs the attached TUI until detach, session end or no panes left.
pub async fn run_tui(
    endpoint: &str,
    session: &str,
    start: StartView,
    opts: AppOptions,
) -> Result<(), String> {
    let mut tui = Tui::attach(endpoint, session, start, opts).await?;
    while tui.step().await {}
    Ok(())
}

/// One wake-up of the event loop; handlers run after the select has released its borrows.
enum Wake {
    Input(Event),
    Stream(Option<(u64, StreamEvent)>),
    Tick,
    /// A delayed element is due: pane output's frame, the shortcut popup, or a sidebar switch.
    Due,
    /// A command that waited on the daemon finished.
    Job(Job),
    /// A history built for a pane in copy mode arrived.
    History(Box<Loaded>),
    /// The command of the open popup produced output or ended.
    Popup(PopupEvent),
}

/// Terminal input, read on its own thread and handed over a channel, with a run of queued mouse
/// drags merged into its last position. crossterm's async `EventStream` takes an internal lock on
/// every poll while its reader thread holds it, so the event loop spun on that lock each time pane
/// output woke it.
struct Input {
    rx: UnboundedReceiver<Event>,
    /// The event that ended the last merged run, handed out next.
    held: Option<Event>,
}

impl Input {
    /// Takes the held event, if any, without waiting: it then completes in the same poll, so
    /// `select!` never drops it.
    async fn recv(&mut self) -> Option<Event> {
        let first = match self.held.take() {
            Some(event) => event,
            None => self.rx.recv().await?,
        };
        let (event, next) = latest_drag(first, || self.rx.try_recv().ok());
        self.held = next;
        Some(event)
    }
}

fn terminal_input() -> UnboundedReceiver<Event> {
    let (tx, rx) = unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    rx
}

/// The attached TUI: the app, its terminal and the buffers the event loop draws with.
struct Tui {
    // Fields drop in declaration order: the output flushes before the terminal guard restores
    // the screen, so `guard` stays last.
    out: BufWriter<std::io::Stdout>,
    body: Vec<u8>,
    cursor: Vec<u8>,
    last_cursor: Vec<u8>,
    tick: tokio::time::Interval,
    keys: Input,
    stream: StreamRx,
    app: App,
    guard: TerminalGuard,
    mouse_captured: bool,
    redraw: bool,
}

impl Tui {
    /// Enters the terminal and attaches the app to the session.
    async fn attach(
        endpoint: &str,
        session: &str,
        start: StartView,
        opts: AppOptions,
    ) -> Result<Self, String> {
        let guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {e}"))?;
        let report = query_terminal(opts.config.style.dim_inactive);
        let (mut app, stream) = App::open(endpoint, session, start, opts).await?;
        app.canvas.set_lr_margins(report.margins);
        app.host.colors = HostColors::from_report(&report);
        app.host.original = app.host.colors;
        let mut tui = Self {
            out: BufWriter::with_capacity(64 * 1024, stdout()),
            body: Vec::with_capacity(64 * 1024),
            cursor: Vec::new(),
            last_cursor: Vec::new(),
            tick: tokio::time::interval(Duration::from_secs(1)),
            keys: Input {
                rx: terminal_input(),
                held: None,
            },
            stream,
            app,
            guard,
            mouse_captured: true,
            redraw: true,
        };
        tui.sync_outer();
        Ok(tui)
    }

    /// One pass of the event loop: draw if due, wait for a wake-up, handle it. Returns false
    /// once the loop should end.
    async fn step(&mut self) -> bool {
        if self.redraw && !self.draw().await {
            return false;
        }
        let wake = self.wait().await;
        let output = matches!(wake, Wake::Stream(_) | Wake::Popup(_));
        let ticked = matches!(wake, Wake::Tick);
        let flow = self.handle(wake).await;
        self.sync_outer();
        let suspended = self.suspend_if_requested();
        sync_mouse_capture(
            &mut self.out,
            self.app.config.mouse,
            &mut self.mouse_captured,
        );
        let forwarded = std::mem::take(&mut self.app.pacing.forwarded);
        self.redraw = needs_redraw(output, forwarded, self.app.pacing.frame_due, Instant::now())
            || suspended
            || self.app.outer.bell_pending;
        if ticked
            && self.app.live_message().is_none()
            && self.app.pacing.frame_due.is_none()
            && !statusline::status_due(
                self.app.config.status_interval,
                self.app.pacing.drawn_at,
                Instant::now(),
            )
        {
            self.redraw = false;
        }
        flow != Flow::Quit
    }

    /// Draws the next frame, sending only what changed. Returns false when nothing is left to show.
    async fn draw(&mut self) -> bool {
        if !self.app.prepare_frame().await && !self.app.switch_after_destroy().await {
            return false;
        }
        self.body.clear();
        self.cursor.clear();
        let _ = frame::draw_body(&mut self.app, &mut self.body);
        let _ = frame::place_cursor(&self.app, &mut self.cursor);
        // A frame with no changes that leaves the cursor where it is would only be bytes for
        // the terminal to parse.
        if !self.body.is_empty() || self.cursor != self.last_cursor {
            let _ = frame::send(&mut self.out, &self.body, &self.cursor);
            std::mem::swap(&mut self.cursor, &mut self.last_cursor);
        }
        self.write_outer();
        self.app.drew(Instant::now());
        true
    }

    /// Waits for input, stream output, the tick or a delayed element coming due.
    async fn wait(&mut self) -> Wake {
        let popup = self.app.hud_deadline().filter(|_| !self.app.hud_visible());
        let due = [
            popup,
            self.app.pacing.frame_due,
            self.app.display_deadline(),
            self.app.sync_deadline(),
        ]
        .into_iter()
        .flatten()
        .min();
        tokio::select! {
            Some(event) = next_event(&mut self.app.mode) => Wake::Popup(event),
            Some(event) = self.keys.recv() => Wake::Input(event),
            item = self.stream.recv() => Wake::Stream(item),
            _ = self.tick.tick() => Wake::Tick,
            _ = sleep_until_opt(due.map(tokio::time::Instant::from_std)) => Wake::Due,
            Some(job) = self.app.job_rx.recv() => Wake::Job(job),
            Some(loaded) = self.app.history_rx.recv() => Wake::History(Box::new(loaded)),
        }
    }

    async fn handle(&mut self, wake: Wake) -> Flow {
        match wake {
            Wake::Input(event) => self.app.on_event(event).await,
            Wake::Stream(item) => {
                let flow = self.app.on_stream(item, &mut self.stream).await;
                flush_passthroughs(&mut self.out, &mut self.app.passthroughs);
                flow
            }
            Wake::Popup(event) => {
                self.app.on_popup(event);
                Flow::Continue
            }
            Wake::Tick => self.app.on_tick().await,
            Wake::Job(job) => self.app.finish_job(job).await,
            Wake::History(loaded) => {
                self.app.finish_history(*loaded);
                Flow::Continue
            }
            Wake::Due => {
                self.app.expire_panes(Instant::now());
                Flow::Continue
            }
        }
    }

    /// Gives the terminal to the shell when `suspend-client` asked for it; returns whether it did.
    fn suspend_if_requested(&mut self) -> bool {
        let suspended = std::mem::take(&mut self.app.chrome.suspend_requested);
        if suspended {
            let _ = self.out.flush();
            if let Err(e) = self.guard.suspend() {
                self.app.say(format!("suspend-client: {e}"));
            }
            self.app.canvas.invalidate();
            self.app.layout.dirty = true;
            self.last_cursor.clear();
            self.mouse_captured = true;
        }
        suspended
    }

    /// Brings the outer terminal's focus reporting and keyboard protocol in line with the options.
    fn sync_outer(&mut self) {
        let keys = self.app.config.terminal.extended_keys != ExtendedKeys::Off;
        let _ = self
            .guard
            .sync(&mut self.out, self.app.config.terminal.focus_events, keys);
        let _ = self.out.flush();
    }

    /// Sends the outer terminal the title and bell that the frame just drew calls for.
    fn write_outer(&mut self) {
        if let Some(title) = self.app.outer_title() {
            let _ = self.guard.set_title(&mut self.out, &title);
        }
        if self.app.take_bell() {
            let _ = self.out.write_all(b"\x07");
        }
        let _ = self.out.flush();
    }
}

/// Turns the terminal's mouse reporting on or off to match `mouse`; `captured` is what it is set to.
pub fn sync_mouse_capture<W: Write>(out: &mut W, want: bool, captured: &mut bool) {
    if want == *captured {
        return;
    }
    if want {
        let _ = crossterm::queue!(out, crossterm::event::EnableMouseCapture);
    } else {
        let _ = crossterm::queue!(out, crossterm::event::DisableMouseCapture);
    }
    let _ = out.flush();
    *captured = want;
}

fn flush_passthroughs<W: Write>(out: &mut W, passthroughs: &mut Vec<Vec<u8>>) {
    if passthroughs.is_empty() {
        return;
    }
    for p in passthroughs.drain(..) {
        let _ = out.write_all(&p);
    }
    let _ = out.flush();
}
