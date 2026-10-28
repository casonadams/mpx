//! Mouse: click focuses a pane; dragging from the click starts a selection; release yanks; wheel scrolls.
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::app::{App, Flow, Mode};
use super::copy::CopyState;
use crate::layout::geometry::Rect;

/// Cell position inside `rect`, clamped to its edges.
fn local_point(rect: Rect, col: u16, row: u16) -> (usize, usize) {
    let x = (col.saturating_sub(rect.x) as usize).min(rect.width.saturating_sub(1) as usize);
    let y = (row.saturating_sub(rect.y) as usize).min(rect.height.saturating_sub(1) as usize);
    (x, y)
}

impl App {
    pub async fn on_mouse(&mut self, ev: MouseEvent) -> Flow {
        if matches!(
            self.mode,
            Mode::Overlay(_) | Mode::Prompt(_) | Mode::Confirm(..)
        ) {
            return Flow::Continue;
        }
        if ev.column < self.sidebar_width() && ev.row < self.viewport.1 {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                self.sidebar_click(ev.row).await;
            }
            return Flow::Continue;
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.mouse_press(ev.column, ev.row).await,
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag(ev.column, ev.row),
            MouseEventKind::Up(MouseButton::Left) => self.mouse_release(),
            MouseEventKind::ScrollUp => self.mouse_scroll_up(),
            MouseEventKind::ScrollDown => self.mouse_scroll_down(),
            _ => {}
        }
        Flow::Continue
    }

    /// Focuses the pane under the pointer. A click inside a pane in copy mode moves its cursor
    /// and drops the selection without touching the scroll position; anywhere else it only
    /// remembers the point, so a later drag can select from it.
    async fn mouse_press(&mut self, col: u16, row: u16) {
        if row >= self.viewport.1 {
            let zoomed = self.zoomed.is_some();
            if let Some(target) = super::status::status_hit(
                &self.session,
                &self.layout.tabs,
                &self.panes,
                zoomed,
                col as usize,
            ) {
                match target {
                    super::status::StatusTarget::Window(idx) => {
                        self.zoomed = None;
                        let _ = self.client.select_window(idx).await;
                        self.layout.dirty = true;
                    }
                    super::status::StatusTarget::Pane(name) => {
                        if name != self.active_pane {
                            self.zoomed = None;
                            let _ = self.client.focus(name).await;
                            self.layout.dirty = true;
                        }
                    }
                }
            }
            return;
        }

        let Some((name, rect)) = self
            .panes
            .iter()
            .find(|(_, rect, _)| rect.contains(col, row))
            .map(|(name, rect, _)| (name.clone(), *rect))
        else {
            return;
        };
        let (x, y) = local_point(rect, col, row);
        if matches!(self.mode, Mode::Sidebar) {
            self.mode = Mode::Normal;
        }
        if name != self.active_pane {
            self.park_copy();
            let _ = self.client.focus(&name).await;
            self.layout.dirty = true;
            self.active_pane = name.clone();
            self.active_rect = Some(rect);
            if let Some(cs) = self.parked_copy.remove(&name) {
                self.mode = Mode::Copy(cs);
            }
        }
        if let Mode::Copy(cs) = &mut self.mode {
            let grid = self.vt.get(&name).map(|vt| vt.grid());
            cs.click(x, y, grid);
        }
        self.press = Some((name, x, y));
    }

    /// Extends the selection, entering copy mode at the press point on the first drag.
    fn mouse_drag(&mut self, col: u16, row: u16) {
        let Some(rect) = self.active_rect else { return };
        if !matches!(self.mode, Mode::Copy(_)) {
            let Some((name, x, y)) = self.press.clone() else {
                return;
            };
            let grid = self.vt.get(&name).map(|vt| vt.grid());
            self.mode = Mode::Copy(CopyState::at_point(x, y, grid));
        }
        if let Mode::Copy(cs) = &mut self.mode {
            let (x, y) = local_point(rect, col, row);
            cs.drag_to(x, y);
        }
    }

    /// Copies the selection when it covers something; the selection stays on screen.
    fn mouse_release(&mut self) {
        self.press = None;
        let (Mode::Copy(cs), Some(vt)) = (&self.mode, self.active_vt()) else {
            return;
        };
        let grid = vt.grid();
        if cs.is_empty_selection(grid) {
            return;
        }
        let text = cs.yank_text(grid);
        if !text.trim().is_empty() {
            self.clipboard.copy(&text);
        }
    }

    fn mouse_scroll_up(&mut self) {
        let hist_len = self.active_vt().map_or(0, |vt| vt.grid().history_len());
        if let Mode::Copy(cs) = &mut self.mode {
            cs.wheel_up(hist_len);
            return;
        }
        let grid = self.active_vt().map(|vt| vt.grid());
        self.mode = Mode::Copy(CopyState::from_wheel(grid, hist_len));
    }

    fn mouse_scroll_down(&mut self) {
        let pane_h = self.active_rect.map_or(24, |r| r.height as usize);
        if let Mode::Copy(cs) = &mut self.mode {
            cs.wheel_down(pane_h);
        }
    }
}
