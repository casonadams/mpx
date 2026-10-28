//! Output pacing: pane readers wait while an attached terminal is behind on output already sent to
//! it, so a flood runs at the speed the terminal can show it, as it would in a real terminal,
//! instead of piling up in front of everything else (a keystroke's echo in another pane included).
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::now_ms;

/// Events queued to one terminal beyond which pane readers wait.
pub const PACE_HIGH: usize = 64;
/// A terminal that has sent nothing for this long is not waited for: a stopped or hung client
/// must never freeze the panes.
pub const PACE_STUCK_MS: u64 = 1_000;
const PACE_POLL: Duration = Duration::from_micros(250);

/// Output chunks a pane may have read but not yet parsed. Its reader waits beyond this, so a flood
/// runs at the speed the daemon parses it and requests to the pane (input, Ctrl-C, kill) never
/// queue behind megabytes of unparsed output.
pub const PANE_INFLIGHT: usize = 64;

/// Whether a pane reader with `unparsed` chunks waiting should wait before reading more.
pub fn pane_full(unparsed: usize) -> bool {
    unparsed >= PANE_INFLIGHT
}

/// Output chunks a pane's reader handed over that its actor has not processed yet.
#[derive(Debug, Default)]
pub struct Inflight(AtomicUsize);

impl Inflight {
    /// A chunk is about to be handed to the actor.
    pub fn add(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// The actor processed a chunk.
    pub fn done(&self) {
        let _ = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
    }

    pub fn full(&self) -> bool {
        pane_full(self.0.load(Ordering::Relaxed))
    }

    /// Blocks the calling reader thread while the actor is behind.
    pub fn wait_for_room(&self) {
        while self.full() {
            std::thread::sleep(PACE_POLL);
        }
    }

    /// `wait_for_room` for readers running on the async runtime.
    pub async fn wait_for_room_async(&self) {
        while self.full() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

/// Whether a terminal with `queued` events waiting, last progressing at `progress_ms`, should hold
/// pane readers back at `now_ms`.
pub fn holds(queued: usize, progress_ms: u64, now_ms: u64) -> bool {
    queued > PACE_HIGH && now_ms.saturating_sub(progress_ms) < PACE_STUCK_MS
}

/// Events delivered to one terminal's queue and not yet written to its socket.
#[derive(Debug, Default)]
pub struct Backlog {
    queued: AtomicUsize,
    progress_ms: AtomicU64,
    /// The terminal is gone. Its sink may outlive it until the next event, which a paused pane
    /// would never produce, so closing must release the readers at once.
    closed: AtomicBool,
}

impl Backlog {
    /// One more event waits for the terminal.
    pub fn queued(&self) {
        self.queued.fetch_add(1, Ordering::Relaxed);
    }

    /// One event reached the terminal's socket at `now_ms`.
    pub fn sent(&self, now_ms: u64) {
        let _ = self
            .queued
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
        self.progress_ms.store(now_ms, Ordering::Relaxed);
    }

    /// The terminal's subscription ended.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    pub fn holds(&self, now_ms: u64) -> bool {
        !self.closed.load(Ordering::Relaxed)
            && holds(
            self.queued.load(Ordering::Relaxed),
            self.progress_ms.load(Ordering::Relaxed),
            now_ms,
        )
    }
}

/// Closes a backlog when dropped, so a subscription that ends any way at all stops pacing.
pub struct CloseOnDrop(pub Option<Arc<Backlog>>);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Some(backlog) = &self.0 {
            backlog.close();
        }
    }
}

/// The backlogs of every attached terminal, shared by a session's pane readers.
#[derive(Debug, Default)]
pub struct Pacing {
    backlogs: Mutex<Vec<Weak<Backlog>>>,
}

impl Pacing {
    /// Starts tracking a terminal's backlog; it stops counting once the returned handle is dropped.
    pub fn track(&self, now_ms: u64) -> Arc<Backlog> {
        let backlog = Arc::new(Backlog::default());
        backlog.progress_ms.store(now_ms, Ordering::Relaxed);
        if let Ok(mut backlogs) = self.backlogs.lock() {
            backlogs.retain(|b| b.strong_count() > 0);
            backlogs.push(Arc::downgrade(&backlog));
        }
        backlog
    }

    /// Whether any live terminal is behind and still catching up.
    pub fn congested(&self, now_ms: u64) -> bool {
        self.backlogs.lock().is_ok_and(|backlogs| {
            backlogs
                .iter()
                .filter_map(Weak::upgrade)
                .any(|b| b.holds(now_ms))
        })
    }

    /// Blocks the calling reader thread while a terminal is behind.
    pub fn wait_for_room(&self) {
        while self.congested(now_ms()) {
            std::thread::sleep(PACE_POLL);
        }
    }
}
