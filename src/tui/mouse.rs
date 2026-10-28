//! Mouse: click focuses a pane and starts a selection; drag extends it; release yanks; wheel scrolls.
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::app::{App, Flow, Mode};
use super::copy::CopyState;
use super::render::copy_to_clipboard;
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

    async fn mouse_press(&mut self, col: u16, row: u16) {
        let Some((name, rect)) = self
            .panes
            .iter()
            .find(|(_, rect, _)| rect.contains(col, row))
            .map(|(name, rect, _)| (name.clone(), *rect))
        else {
            return;
        };
        if name != self.active_pane {
            let _ = self.client.focus(&name).await;
            self.layout.dirty = true;
        }
        let (x, y) = local_point(rect, col, row);
        let grid = self.vt.get(&name).map(|vt| vt.grid());
        let cs = CopyState::at_point(x, y, grid);
        self.mode = Mode::Copy(cs);
    }

    fn mouse_drag(&mut self, col: u16, row: u16) {
        let Some(rect) = self.active_rect else { return };
        if let Mode::Copy(cs) = &mut self.mode {
            let (x, y) = local_point(rect, col, row);
            cs.cursor_x = x;
            cs.cursor_y = y;
        }
    }

    /// Copies the selection when it covers something; the selection stays on screen.
    fn mouse_release(&mut self) {
        let (Mode::Copy(cs), Some(vt)) = (&self.mode, self.active_vt()) else {
            return;
        };
        let grid = vt.grid();
        if cs.is_empty_selection(grid) {
            return;
        }
        let text = cs.yank_text(grid);
        if !text.trim().is_empty() {
            copy_to_clipboard(&text);
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
