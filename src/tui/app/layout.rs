use std::collections::{HashMap, HashSet};

use super::state::{App, Mode};
use crate::config::StatusPosition;
use crate::keys::TermModes;
use crate::layout::geometry::Rect;
use crate::tui::panes::{resized_elsewhere, sync_dims};
use crate::tui::statusline;
use crate::vt::VtScreen;

impl App {
    /// Fetches the layout and pane sizes for the next frame. Returns false when nothing is left to show.
    pub async fn prepare_frame(&mut self) -> bool {
        let (w, h) = self
            .launch
            .fixed_size
            .unwrap_or_else(|| crossterm::terminal::size().unwrap_or((80, 24)));
        let status_lines = if self.config.status {
            self.config.status_lines
        } else {
            0
        };
        let rows = statusline::rows(
            h,
            status_lines,
            self.config.status_position == StatusPosition::Top,
        );
        if rows != self.rows {
            self.rows = rows;
            self.canvas.invalidate();
        }
        self.viewport = (w, rows.pane_h);
        self.report_size(w, rows.pane_h).await;
        if !self
            .layout
            .sync(&mut self.client, (0, rows.pane_top), (w, rows.pane_h))
            .await
        {
            return false;
        }
        let focused = self
            .layout
            .raw
            .iter()
            .find(|(.., focused)| *focused)
            .map(|(name, ..)| name.clone())
            .unwrap_or_default();
        self.follow_focus(focused);
        self.refresh_outer((w, h));
        let shown_before: Vec<String> = self.panes.iter().map(|(name, ..)| name.clone()).collect();
        self.panes = if self.zoomed() {
            // Zoomed: the focused pane alone fills the pane area.
            self.layout
                .raw
                .iter()
                .filter(|(.., focused)| *focused)
                .map(|(name, ..)| {
                    (
                        name.clone(),
                        Rect::new(0, rows.pane_top, w, rows.pane_h),
                        true,
                    )
                })
                .collect()
        } else {
            self.layout.raw.clone()
        };
        self.active_rect = self.panes.iter().find(|(.., f)| *f).map(|(_, r, _)| *r);
        self.marked_rect = self
            .layout
            .marked_pane()
            .and_then(|m| self.panes.iter().find(|(name, ..)| name == m))
            .map(|(_, r, _)| *r);
        self.catch_up_released().await;
        self.refresh_newly_shown(&shown_before).await;
        if self.focus_pane.as_deref() != Some(self.active_pane.as_str()) {
            self.recheck_size(&self.active_pane.clone()).await;
        }
        // While a border is dragged, panes are drawn at their new places as they are: resizing
        // their terminals and rebuilding their screens waits for the release.
        if self.interaction.border_drag.is_none() {
            self.sync_pane_sizes().await;
        }
        self.route_focus().await;
        true
    }

    async fn sync_pane_sizes(&mut self) {
        let before: HashMap<String, (usize, usize)> = self
            .vt
            .iter()
            .map(|(name, vt)| (name.clone(), (vt.grid().history_len(), vt.grid().cols)))
            .collect();
        sync_dims(
            &mut self.client,
            &self.panes,
            &mut self.pane_dims,
            &mut self.vt,
            &mut self.expected,
        )
        .await;
        self.keep_copy_positions(&before);
    }

    /// Copy-mode positions of panes resized by `sync_pane_sizes` follow their lines through the
    /// rebuilt screens; `before` holds each pane's history length and width before the resize.
    fn keep_copy_positions(&mut self, before: &HashMap<String, (usize, usize)>) {
        for (name, &(old_hist, old_cols)) in before {
            let Some(grid) = self.vt.get(name).map(VtScreen::grid) else {
                continue;
            };
            let reflowed = grid.cols != old_cols;
            let copy = match &mut self.mode {
                Mode::Copy(cs) if *name == self.active_pane => Some(cs),
                _ => self.parked_copy.get_mut(name),
            };
            if let Some(cs) = copy {
                cs.resized(old_hist, grid, reflowed);
            }
        }
    }

    /// A pane coming into focus gets its size checked: if something else resized its terminal,
    /// the next sync sends the right size again and the program reflows.
    /// Only on focus: two clients of different sizes attached to one session would otherwise
    /// take a pane back and forth.
    async fn recheck_size(&mut self, name: &str) {
        if let Ok(info) = self.client.status(name).await {
            for name in resized_elsewhere(&self.pane_dims, &[info]) {
                self.pane_dims.remove(&name);
            }
        }
    }

    /// Sends focus-out to the previously active pane and focus-in to the new one, where enabled.
    async fn route_focus(&mut self) {
        if self.active_pane.is_empty()
            || self.focus_pane.as_deref() == Some(self.active_pane.as_str())
        {
            return;
        }
        if let Some(old) = self.focus_pane.take() {
            self.send_focus(&old, false).await;
        }
        let active = self.active_pane.clone();
        self.send_focus(&active, true).await;
        self.focus_pane = Some(active);
    }

    /// Reports focus in or out to pane `name`, when the pane asked for reports.
    pub(in crate::tui) async fn send_focus(&mut self, name: &str, focused: bool) {
        if name.is_empty() || !self.modes_of(name).focus_events || self.attachment.terminal.readonly
        {
            return;
        }
        let text = String::from_utf8_lossy(crate::keys::focus_event(focused)).into_owned();
        let _ = self.client.send_input(name, &text).await;
    }

    /// Width and height of the area the panes are laid out in, right of the sidebar.
    pub fn pane_viewport(&self) -> (u16, u16) {
        self.viewport
    }

    /// Whether the active window is zoomed (the daemon's flag, as the layout reports it).
    pub fn zoomed(&self) -> bool {
        self.layout.tabs.iter().any(|t| t.is_active && t.zoomed)
    }

    pub fn modes_of(&self, name: &str) -> TermModes {
        self.vt.get(name).map(|vt| vt.modes()).unwrap_or_default()
    }

    /// Makes `focused` the active pane. Copy mode belongs to a pane: leaving the pane parks it and
    /// arriving at a pane with parked copy state resumes it.
    fn follow_focus(&mut self, focused: String) {
        if focused != self.active_pane {
            self.park_copy();
        }
        self.active_pane = focused;
        if matches!(self.mode, Mode::Normal)
            && let Some(cs) = self.parked_copy.remove(&self.active_pane)
        {
            self.mode = Mode::Copy(cs);
        }
        let live: HashSet<&str> = self.layout.raw.iter().map(|(n, ..)| n.as_str()).collect();
        self.parked_copy
            .retain(|name, _| live.contains(name.as_str()));
    }

    /// Moves an active copy mode out of the foreground, keyed by the active pane.
    pub fn park_copy(&mut self) {
        if matches!(self.mode, Mode::Copy(_))
            && let Mode::Copy(cs) = std::mem::replace(&mut self.mode, Mode::Normal)
        {
            self.parked_copy.insert(self.active_pane.clone(), cs);
        }
    }

    pub fn active_vt(&self) -> Option<&VtScreen> {
        self.vt.get(&self.active_pane)
    }
}
