//! Scrollback built off the event loop. A pane's history is its log replayed through the parser,
//! which takes long enough on a large log to stall the first scroll or `copy-mode`. The focused
//! pane's history is replayed on a blocking thread while it is idle, and copy mode takes it and
//! feeds only the output since.
use tokio::task::JoinHandle;

use super::app::{App, Mode};
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

    /// Forgets every history built or being built for pane `name`: its process or its log is no
    /// longer the one they were built from (respawned, exited, or another session's pane).
    pub(super) fn forget_history(&mut self, name: &str) {
        self.warm.take_if(|w| w.name == name);
        self.warm_next.take_if(|w| w.name == name);
    }

    /// Forgets the histories of every pane, as a switch to another session does.
    pub(super) fn forget_all_history(&mut self) {
        self.released.clear();
        self.warm = None;
        self.warm_next = None;
    }
}
