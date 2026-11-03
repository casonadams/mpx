//! What the app shows beyond the panes' own cells: the outer terminal's title and bell, status-row
//! messages for bells and window activity, and the wait a pane's drawing gets while it is in the
//! middle of a synchronized update (DEC 2026).
use std::time::{Duration, Instant};

use super::app::{App, Mode};
use super::frame;

/// Longest a pane's drawing waits for a synchronized update it has begun but not ended.
pub const SYNC_HOLD: Duration = Duration::from_secs(1);

impl App {
    /// Records the bells panes rang since the last call. Each rings the outer terminal or, with
    /// `visual-bell`, shows on the status row, when `bell-action` admits it.
    pub(super) fn ring_bells(&mut self) {
        for pane in std::mem::take(&mut self.bells) {
            let shown = self.layout.raw.iter().any(|(name, ..)| *name == pane);
            if !self.config.bell_action.rings(shown) {
                continue;
            }
            if self.config.visual_bell {
                let window = self.window_of(&pane);
                self.say(format!("Bell in window {window}"));
            } else {
                self.bell_pending = true;
            }
        }
    }

    /// Whether the outer terminal should ring now; the bell is taken.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending)
    }

    /// The outer terminal's title when `set-titles` is on and it differs from the last one set.
    pub(super) fn outer_title(&mut self) -> Option<String> {
        if !self.config.set_titles {
            return None;
        }
        let title: String = frame::expand_active(self, &self.config.set_titles_string)
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if self.last_title.as_deref() == Some(title.as_str()) {
            return None;
        }
        self.last_title = Some(title.clone());
        Some(title)
    }

    /// Per frame: a popup is fitted to the screen, panes that began a synchronized update are timed,
    /// and windows that gained activity are announced once when `visual-activity` is on.
    pub(super) fn refresh_outer(&mut self, screen: (u16, u16)) {
        if let Mode::Popup(popup) = &mut self.mode {
            popup.fit(screen);
        }
        let now = Instant::now();
        let syncing: Vec<String> = self
            .vt
            .iter()
            .filter(|(_, vt)| vt.sync_output())
            .map(|(name, _)| name.clone())
            .collect();
        self.sync_since.retain(|name, _| syncing.contains(name));
        for name in syncing {
            self.sync_since.entry(name).or_insert(now);
        }
        let fresh: Vec<String> = self
            .layout
            .tabs
            .iter()
            .filter(|tab| tab.has_activity && !tab.is_active)
            .filter(|tab| !self.activity_windows.contains(&tab.index))
            .map(|tab| tab.name.clone())
            .collect();
        self.activity_windows = self
            .layout
            .tabs
            .iter()
            .filter(|tab| tab.has_activity && !tab.is_active)
            .map(|tab| tab.index)
            .collect();
        if self.config.visual_activity {
            for name in fresh {
                self.say(format!("Activity in window {name}"));
            }
        }
    }

    /// Whether pane `name` is in a synchronized update that has not ended and has not waited out
    /// `SYNC_HOLD`: its drawing waits, so the frame shows its last complete state.
    pub(super) fn sync_held(&self, name: &str) -> bool {
        self.sync_since
            .get(name)
            .is_some_and(|since| since.elapsed() < SYNC_HOLD)
    }

    /// The name of the window that holds pane `name`, empty when it is unknown.
    fn window_of(&self, pane: &str) -> String {
        let index = self
            .nodes
            .iter()
            .find(|node| node.name == pane)
            .and_then(|node| node.window);
        index
            .and_then(|i| self.layout.tabs.iter().find(|tab| tab.index == i))
            .map(|tab| tab.name.clone())
            .unwrap_or_default()
    }
}
