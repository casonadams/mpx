use std::time::Instant;

use tokio::sync::mpsc;

use super::pacing::{FRAMES_PER_BUDGET, TYPING_WINDOW, next_frame};
use super::state::{App, AppOptions, AttachOpts, Flow, Mode, StartView, StreamRx};
use crate::client::Client;
use crate::protocol::{NewWindowOpts, SpawnOpts, StreamEvent};
use crate::tui::keymap;
use crate::tui::status::{local_utc_offset, restored_notice};

/// Stream events the app holds before its subscription stops reading the daemon. Small, so a pane
/// flooding faster than the app draws is paced by the daemon rather than queued here, ahead of a
/// keystroke's echo in another pane.
const STREAM_QUEUE: usize = 64;
/// Times output handling yields to let the rest of a burst arrive before its frame.
const BURST_YIELDS: usize = 4;

impl App {
    /// Attaches to the session: ensures a running window, subscribes to output and builds the app.
    pub async fn open(
        endpoint: &str,
        session: &str,
        _start: StartView,
        opts: AppOptions,
    ) -> Result<(Self, StreamRx), String> {
        let keymap = keymap::resolve(&opts.config)?;
        let attach = opts.attach;
        let mut client = Client::connect(endpoint).await?;
        let sub_client = Client::connect(endpoint).await?;
        let started = ensure_running_window(&mut client).await;
        let (tx, rx) = mpsc::channel(STREAM_QUEUE);
        let mut app = Self::new(endpoint, session, client, keymap, tx, opts);
        app.report("new-window", started);
        app.subscribe_as(sub_client, attach);
        app.refresh_attached().await;
        app.apply_bindings().await;
        app.refresh_nodes().await;
        if let Ok(info) = app.client.info().await {
            // The daemon's name wins over the socket's: `rename-session` changes one, not the other.
            app.attachment.times = crate::format::SessionTimes {
                started_ms: info.created_ms,
                restored_ms: info.restored_ms,
            };
            app.session = info.name;
            if let Some(restored_ms) = info.restored_ms {
                app.say(restored_notice(restored_ms, local_utc_offset(restored_ms)));
            }
        }
        Ok((app, rx))
    }

    /// Replaces the event subscription with one bound to `client`'s session, as this client.
    pub fn subscribe(&mut self, client: Client) {
        self.subscribe_as(client, AttachOpts::default());
    }

    /// Replaces the event subscription with one bound to `client`'s session, registered as this
    /// client (with `exclusive`, the session's other clients are detached first, and with `hangup`
    /// their parents are hung up).
    pub(super) fn subscribe_as(&mut self, client: Client, attach: AttachOpts) {
        if let Some(task) = self.sub.task.take() {
            task.abort();
        }
        self.sub.generation += 1;
        let generation = self.sub.generation;
        let tx = self.sub.tx.clone();
        let terminal = self.attachment.terminal.clone();
        self.sub.task = Some(tokio::spawn(async move {
            let _ = client
                .stream_as_client(
                    terminal,
                    attach.exclusive,
                    attach.hangup,
                    tx,
                    move |event| (generation, event),
                )
                .await;
        }));
    }

    pub async fn refresh_nodes(&mut self) {
        if let Ok(nodes) = self.client.list().await {
            self.nodes = nodes;
        }
    }

    /// Applies stream events from the current subscription. The session ending is noticed when the
    /// next frame finds no panes in the layout.
    pub async fn on_stream(
        &mut self,
        first: Option<(u64, StreamEvent)>,
        rx: &mut StreamRx,
    ) -> Flow {
        let Some(first) = first else {
            return Flow::Continue;
        };
        let mut batch = vec![first];
        drain_into(rx, &mut batch);
        // The rest of a burst (a pager page arrives in several reads) is often already on its way:
        // yielding lets it land in this frame instead of a later one, at no timer cost.
        for _ in 0..BURST_YIELDS {
            tokio::task::yield_now().await;
            if !drain_into(rx, &mut batch) {
                break;
            }
        }
        let (runs, detached) = take_control_events(&mut batch);
        if detached {
            return Flow::Quit;
        }
        let echo = batch.iter().any(|(_, event)| {
            matches!(event, StreamEvent::Output { name, .. } if *name == self.active_pane)
        });
        // Output of panes nobody sees changes nothing on screen: no frame for it.
        let shown = batch.iter().any(|(_, event)| match event {
            StreamEvent::Output { name, .. } => self.keeps_screen(name),
            _ => true,
        });
        self.apply_batch(batch).await;
        for line in &runs {
            if self.run_forwarded(line).await == Flow::Quit {
                return Flow::Quit;
            }
        }
        if !shown && runs.is_empty() {
            return Flow::Continue;
        }
        let now = Instant::now();
        // The focused pane's output right after a keypress is likely its response: it goes out at
        // once, whatever other panes are flooding, for a few frames per keypress.
        let typing = echo
            && self.pacing.key_frames > 0
            && self
                .pacing
                .last_key
                .is_some_and(|at| now.saturating_duration_since(at) < TYPING_WINDOW);
        if typing {
            self.pacing.key_frames -= 1;
        }
        // The budget window starts at the oldest of the frames drawn in it, once it is full.
        let frames = &self.pacing.output_frames;
        let window_start = frames
            .front()
            .copied()
            .filter(|_| frames.len() == FRAMES_PER_BUDGET);
        self.pacing.frame_due = Some(if typing {
            now
        } else {
            next_frame(window_start, now, self.pacing.frame_due)
        });
        Flow::Continue
    }

    pub async fn on_tick(&mut self) -> Flow {
        self.layout.dirty = true;
        self.refresh_nodes().await;
        self.refresh_attached().await;
        if matches!(self.mode, Mode::Overlay(_)) {
            self.refresh_overlay().await;
        }
        Flow::Continue
    }

    /// Applies the events of the current subscription.
    async fn apply_batch(&mut self, batch: Vec<(u64, StreamEvent)>) {
        for (generation, event) in batch {
            if generation != self.sub.generation {
                continue;
            }
            if self.apply_event(event).await {
                self.layout.dirty = true;
            }
        }
        self.after_input().await;
    }
}

/// Creates a shell window when the session has no running pane.
async fn ensure_running_window(client: &mut Client) -> Result<(), String> {
    let has_running = client
        .list()
        .await
        .map(|nodes| nodes.iter().any(|n| n.is_running()))
        .unwrap_or(false);
    if has_running {
        return Ok(());
    }
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let spawn = SpawnOpts {
        command: String::new(),
        pty: true,
        cwd,
        ..SpawnOpts::default()
    };
    client
        .new_window(NewWindowOpts {
            name: None,
            spawn,
            focus: true,
        })
        .await
        .map(|_| ())
}

/// Moves every event already queued into `batch`; returns whether there was any.
fn drain_into(rx: &mut StreamRx, batch: &mut Vec<(u64, StreamEvent)>) -> bool {
    let before = batch.len();
    while let Ok(more) = rx.try_recv() {
        batch.push(more);
    }
    batch.len() > before
}

/// Removes the run and detach events from `batch`: the lines to run, and whether the client was
/// detached.
fn take_control_events(batch: &mut Vec<(u64, StreamEvent)>) -> (Vec<String>, bool) {
    let mut runs = Vec::new();
    let mut detached = false;
    batch.retain(|(_, event)| match event {
        StreamEvent::Detached => {
            detached = true;
            false
        }
        StreamEvent::Run { line } => {
            runs.push(line.clone());
            false
        }
        _ => true,
    });
    (runs, detached)
}
