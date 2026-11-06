//! Gap-free pane output: each screen tracks the byte offset it has absorbed, and every event is
//! checked against that offset. Missing bytes are fetched from the daemon's log.

use super::app::App;
use super::panes::{has_scrollback, load_pane};
use crate::protocol::StreamEvent;
use crate::vt::{Alert, VtScreen};

/// Largest gap a pane coming into view catches up on by reading it: three quarters of the log a
/// pane keeps, so the bytes are still there. Reading them replays the program's output exactly;
/// a rebuild from the log cannot once the log has dropped what the program painted only once.
const CATCH_UP_MAX: u64 = (crate::buffer::RING_CAPACITY as u64) * 3 / 4;

/// Size of a screen whose pane has not been sized yet.
const DEFAULT_SIZE: (u16, u16) = (80, 24);

/// What to do with one output chunk, given the offset the pane's screen expects next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPlan {
    /// Continues the stream exactly: feed the chunk.
    Feed,
    /// Already covered by the screen (or the pane has no baseline yet, and its load will cover it).
    Skip,
    /// Bytes are missing before the chunk, or it overlaps the expected offset: fetch from the log.
    Fetch,
}

/// Decides how a chunk spanning `offset..next` applies to a screen expecting `expected`.
pub fn plan_output(expected: Option<u64>, offset: u64, next: u64) -> OutputPlan {
    match expected {
        None => OutputPlan::Skip,
        Some(expected) if next <= expected => OutputPlan::Skip,
        Some(expected) if offset == expected => OutputPlan::Feed,
        Some(_) => OutputPlan::Fetch,
    }
}

impl App {
    /// Applies one event from the current subscription. Returns true when the layout must be refetched.
    pub async fn apply_event(&mut self, event: StreamEvent) -> bool {
        match event {
            StreamEvent::Output {
                name,
                data,
                offset,
                next,
            } => {
                self.apply_output(&name, &data, offset, next).await;
                false
            }
            StreamEvent::Exit { name, .. } => {
                self.vt.remove(&name);
                self.expected.remove(&name);
                self.pane_dims.remove(&name);
                true
            }
            // Panes created or rearranged from outside (an agent's split, `mpx focus`) show at
            // once, not at the next tick.
            StreamEvent::Spawned { .. } | StreamEvent::Layout => true,
            StreamEvent::Respawned { name } => {
                self.forget_screen(&name);
                true
            }
            StreamEvent::Bindings => {
                self.apply_bindings().await;
                true
            }
            StreamEvent::Lagged => {
                self.resync_all().await;
                true
            }
            StreamEvent::Refresh => {
                self.canvas.invalidate();
                true
            }
            // `on_stream` ends the TUI or runs the command before events reach here.
            StreamEvent::Detached | StreamEvent::Run { .. } => false,
        }
    }

    /// Output for a pane in copy mode is not applied: the view stays as it was entered, like
    /// tmux's, and leaving copy mode brings the screen up to date from the pane's log. Output
    /// for a pane that is not shown is not applied either: its screen keeps its offset and
    /// catches up when the pane comes into view.
    async fn apply_output(&mut self, name: &str, data: &str, offset: u64, next: u64) {
        if !self.keeps_screen(name) || self.in_copy_mode(name) {
            return;
        }
        match plan_output(self.expected.get(name).copied(), offset, next) {
            OutputPlan::Skip => {}
            OutputPlan::Feed => self.feed(name, data, next),
            OutputPlan::Fetch => {
                self.catch_up(name).await;
                self.apply_after_fetch(name, data, offset, next).await;
            }
        }
    }

    /// Applies a chunk once the log has been read up to the expected offset; a chunk still out of
    /// step after that means the screen cannot be patched, so the pane is rebuilt.
    async fn apply_after_fetch(&mut self, name: &str, data: &str, offset: u64, next: u64) {
        match plan_output(self.expected.get(name).copied(), offset, next) {
            OutputPlan::Feed => self.feed(name, data, next),
            OutputPlan::Skip => {}
            OutputPlan::Fetch => self.resync_pane(name).await,
        }
    }

    /// Whether the TUI keeps `name`'s screen up to date: it is on screen, or holds a parked copy
    /// view. Other panes are rebuilt from their log when they come into view, so output nobody
    /// sees costs no parsing.
    pub fn keeps_screen(&self, name: &str) -> bool {
        self.panes.iter().any(|(pane, ..)| pane == name)
            || self.parked_copy.contains_key(name)
    }

    /// Panes that just came into view may have missed output while hidden (the daemon sends only
    /// the active window's): each is brought up to date.
    pub async fn refresh_newly_shown(&mut self, before: &[String]) {
        let fresh: Vec<String> = self
            .panes
            .iter()
            .map(|(name, ..)| name.clone())
            .filter(|name| !before.contains(name))
            .collect();
        for name in fresh {
            self.bring_up_to_date(&name).await;
        }
    }

    /// Nothing missed keeps the screen as it is; a gap the log still holds is fetched and fed; a
    /// larger one rebuilds the pane from its log (with scrollback for a parked copy view).
    async fn bring_up_to_date(&mut self, name: &str) {
        if let Some(expected) = self.expected.get(name).copied() {
            let Ok((_, end)) = self.client.logs(name, None, false, Some(u64::MAX)).await else {
                return;
            };
            if end <= expected {
                return;
            }
            if end - expected <= CATCH_UP_MAX {
                self.catch_up(name).await;
                return;
            }
        }
        if self.parked_copy.contains_key(name) {
            self.resync_pane(name).await;
        } else {
            self.forget_screen(name);
        }
    }

    /// Drops a hidden pane's screen; the next frame that shows it reloads it at its size.
    fn forget_screen(&mut self, name: &str) {
        self.vt.remove(name);
        self.expected.remove(name);
        self.pane_dims.remove(name);
    }

    fn feed(&mut self, name: &str, data: &str, next: u64) {
        let vt = self
            .vt
            .entry(name.to_string())
            .or_insert_with(|| VtScreen::live(DEFAULT_SIZE.0 as usize, DEFAULT_SIZE.1 as usize));
        vt.feed(data.as_bytes());
        if vt.take_alerts().contains(&Alert::Bell) {
            self.outer.bells.push(name.to_string());
        }
        let pass = vt.take_passthroughs();
        if self.config.allow_passthrough.allows(name == self.active_pane) {
            for sequence in &pass {
                self.host.colors.follow(&self.host.original, sequence);
            }
            self.passthroughs.extend(pass);
        }
        self.expected.insert(name.to_string(), next);
    }

    /// Feeds the bytes a pane is missing, read from the log since its expected offset.
    async fn catch_up(&mut self, name: &str) {
        let Some(since) = self.expected.get(name).copied() else {
            return;
        };
        if let Ok((data, next)) = self.client.logs(name, None, false, Some(since)).await {
            if let Some(vt) = self.vt.get_mut(name) {
                vt.feed(data.as_bytes());
                let _ = vt.take_passthroughs();
            }
            self.expected.insert(name.to_string(), next);
        }
    }

    /// Rebuilds one pane's screen from its full log at its current size, keeping scrollback if it
    /// had some.
    async fn resync_pane(&mut self, name: &str) {
        let scrollback = self.vt.get(name).is_some_and(has_scrollback);
        self.reload_pane(name, scrollback).await;
    }

    async fn reload_pane(&mut self, name: &str, scrollback: bool) {
        let size = self.pane_dims.get(name).copied().unwrap_or(DEFAULT_SIZE);
        if let Some((mut screen, next)) = load_pane(&mut self.client, name, size, scrollback).await {
            let _ = screen.take_passthroughs();
            self.vt.insert(name.to_string(), screen);
            self.expected.insert(name.to_string(), next);
        }
    }

    /// Gives `name` its scrollback for copy mode, rebuilt from the pane's log: live panes keep
    /// none in the TUI.
    pub async fn load_scrollback(&mut self, name: &str) {
        if !self.vt.get(name).is_some_and(has_scrollback) {
            self.reload_pane(name, true).await;
        }
    }

    /// Ends `name`'s frozen copy-mode view: its scrollback is dropped now and the output that
    /// arrived meanwhile is fed from the log before the next frame, instead of replaying the
    /// whole log.
    pub fn release_scrollback(&mut self, name: &str) {
        if let Some(vt) = self.vt.get_mut(name) {
            vt.grid_mut().set_history_limit(0);
            self.released.push(name.to_string());
        }
    }

    /// Brings the screens just released from copy mode up to date.
    pub async fn catch_up_released(&mut self) {
        for name in std::mem::take(&mut self.released) {
            self.bring_up_to_date(&name).await;
        }
    }

    /// After the subscriber lagged: every shown pane is rebuilt from its log. Panes that are not
    /// shown keep their offset and catch up when they come into view.
    async fn resync_all(&mut self) {
        for name in self.shown_screens() {
            self.resync_pane(&name).await;
        }
    }

    /// Panes with a screen the TUI keeps up to date.
    fn shown_screens(&self) -> Vec<String> {
        let screens = self.expected.keys();
        screens.filter(|name| self.keeps_screen(name)).cloned().collect()
    }

}
