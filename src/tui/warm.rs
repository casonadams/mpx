//! Scrollback built off the event loop. A pane's history is its log replayed through the parser,
//! which takes long enough on a large log to stall the first scroll or `copy-mode`. The focused
//! pane's history is replayed on a blocking thread while it is idle, and copy mode takes it and
//! feeds only the output since. A wheel scroll that finds none ready enters copy mode at once on
//! the screen it has and builds the history the same way, applying the queued scroll on arrival.
use tokio::task::JoinHandle;

use super::app::{App, Mode};
use super::copy::CopyState;
use super::panes::{has_scrollback, load_pane};
use super::stream::{CATCH_UP_MAX, DEFAULT_SIZE};
use crate::client::Client;
use crate::vt::VtScreen;

/// Output past a built history after which an idle tick builds it again, so the output copy mode
/// must feed stays small on a pane that keeps printing.
const REBUILD_AFTER: u64 = 256 * 1024;

type Built = Option<(VtScreen, u64)>;

/// Builds pane `name`'s screen with `history` lines of scrollback on a blocking thread, over a
/// connection of its own.
fn build(endpoint: &str, name: &str, size: (u16, u16), history: usize) -> JoinHandle<Built> {
    let (endpoint, pane) = (endpoint.to_string(), name.to_string());
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        runtime.block_on(async move {
            let mut client = Client::connect(&endpoint).await.ok()?;
            load_pane(&mut client, &pane, size, history).await
        })
    })
}

/// The history of one pane at one size, being built or built, from the log up to `offset`.
pub struct Warm {
    name: String,
    size: (u16, u16),
    offset: u64,
    task: JoinHandle<Built>,
}

impl Warm {
    fn start(endpoint: &str, name: &str, size: (u16, u16), offset: u64, history: usize) -> Self {
        Self {
            name: name.to_string(),
            size,
            offset,
            task: build(endpoint, name, size, history),
        }
    }

    fn is_for(&self, name: &str, size: (u16, u16)) -> bool {
        self.name == name && self.size == size
    }
}

/// A pane in copy mode whose history is still being built: the wheel notches that came in the
/// meantime wait here and are applied when it arrives.
pub struct Loading {
    name: String,
    pending: usize,
}

/// A finished history build for a pane in copy mode.
pub struct Loaded {
    name: String,
    built: Built,
}

impl App {
    /// Keeps the focused pane's history built ahead: called on each tick. A stale history stays
    /// usable while its replacement builds beside it, so copy mode never waits on a rebuild.
    pub fn warm_scrollback(&mut self) {
        if self
            .warm_next
            .as_ref()
            .is_some_and(|w| w.task.is_finished())
        {
            self.warm = self.warm_next.take();
        }
        let name = self.active_pane.clone();
        if self.warm_next.is_some()
            || matches!(self.mode, Mode::Copy(_))
            || self.vt.get(&name).is_none_or(has_scrollback)
        {
            return;
        }
        let Some(size) = self.pane_dims.get(&name).copied() else {
            return;
        };
        let offset = self.expected.get(&name).copied().unwrap_or(0);
        let history = self.config.session.scrollback_lines;
        match &self.warm {
            Some(warm) if warm.is_for(&name, size) => {
                if warm.task.is_finished() && offset.saturating_sub(warm.offset) > REBUILD_AFTER {
                    self.warm_next =
                        Some(Warm::start(&self.endpoint, &name, size, offset, history));
                }
            }
            _ => self.warm = Some(Warm::start(&self.endpoint, &name, size, offset, history)),
        }
    }

    /// Whether the focused pane's history is built and waiting for copy mode.
    pub fn scrollback_warm(&self) -> bool {
        let size = self
            .pane_dims
            .get(&self.active_pane)
            .copied()
            .unwrap_or(DEFAULT_SIZE);
        self.warm
            .as_ref()
            .is_some_and(|w| w.is_for(&self.active_pane, size) && w.task.is_finished())
    }

    /// Gives pane `name` the history built ahead, brought up to date with the log. False when
    /// none was built for the pane at its size, or the log has moved too far past it.
    pub(super) async fn adopt_warm(&mut self, name: &str) -> bool {
        let size = self.pane_dims.get(name).copied().unwrap_or(DEFAULT_SIZE);
        let next_ready = self
            .warm_next
            .as_ref()
            .is_some_and(|w| w.is_for(name, size) && w.task.is_finished());
        let chosen = if next_ready {
            self.warm_next.take()
        } else {
            self.warm.take_if(|w| w.is_for(name, size))
        };
        let Some(warm) = chosen.or_else(|| self.warm_next.take_if(|w| w.is_for(name, size))) else {
            return false;
        };
        self.warm_next = None;
        let Some((screen, next)) = warm.task.await.ok().flatten() else {
            return false;
        };
        let Ok((_, end)) = self.client.logs(name, None, false, Some(u64::MAX)).await else {
            return false;
        };
        if end.saturating_sub(next) > CATCH_UP_MAX {
            return false;
        }
        self.vt.insert(name.to_string(), screen);
        self.expected.insert(name.to_string(), next);
        self.catch_up(name).await;
        true
    }

    /// Enters copy mode for a wheel scroll on the screen the pane has now, and builds its history
    /// in the background. The notch waits in `loading` and is applied by `finish_history`.
    pub(super) fn enter_copy_loading(&mut self, name: &str) {
        let size = self.pane_dims.get(name).copied().unwrap_or(DEFAULT_SIZE);
        let task = build(
            &self.endpoint,
            name,
            size,
            self.config.session.scrollback_lines,
        );
        let (tx, pane) = (self.history_tx.clone(), name.to_string());
        tokio::spawn(async move {
            let built = task.await.ok().flatten();
            let _ = tx.send(Loaded { name: pane, built });
        });
        let mut cs = CopyState::enter(self.vt.get(name).map(|vt| vt.grid()));
        cs.wheel_exit = true;
        self.mode = Mode::Copy(cs);
        self.loading = Some(Loading {
            name: name.to_string(),
            pending: 1,
        });
    }

    /// Whether the history of `name`, in copy mode, is still being built.
    pub(super) fn is_loading(&self, name: &str) -> bool {
        self.loading.as_ref().is_some_and(|l| l.name == name)
    }

    /// Holds one more wheel notch up for the history being built.
    pub(super) fn queue_wheel_up(&mut self) {
        if let Some(loading) = &mut self.loading {
            loading.pending += 1;
        }
    }

    /// Takes back one queued wheel notch for a notch down. False when none is left to take back,
    /// so the notch scrolls to the live screen as usual.
    pub(super) fn unqueue_wheel_up(&mut self) -> bool {
        match &mut self.loading {
            Some(loading) if loading.pending > 1 => {
                loading.pending -= 1;
                true
            }
            Some(loading) => {
                loading.pending = 0;
                false
            }
            None => false,
        }
    }

    /// Forgets the history being built for `name`: its copy mode ended before it arrived.
    pub(super) fn cancel_loading(&mut self, name: &str) {
        if self.is_loading(name) {
            self.loading = None;
        }
    }

    /// Gives the pane in copy mode the history that just arrived: its screen is replaced, the
    /// selection anchor moves down by the history added, and the queued notches scroll back.
    pub fn finish_history(&mut self, loaded: Loaded) {
        let Some(loading) = self.loading.take_if(|l| l.name == loaded.name) else {
            return;
        };
        let Some((screen, next)) = loaded.built else {
            return;
        };
        if self.active_pane != loaded.name {
            return;
        }
        let Mode::Copy(cs) = &mut self.mode else {
            return;
        };
        let hist_len = screen.grid().history_len();
        cs.anchor_abs_y += hist_len;
        for _ in 0..loading.pending {
            cs.wheel_up(hist_len);
        }
        self.vt.insert(loaded.name.clone(), screen);
        self.expected.insert(loaded.name, next);
    }
}
