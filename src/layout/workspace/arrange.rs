use super::{Window, Workspace};
use crate::layout::geometry::Rect;
use crate::layout::tree::Divider;
use crate::protocol::{LayoutPreset, Side};

impl Workspace {
    /// Applies a preset to the active window; `None` cycles.
    pub fn select_layout(&mut self, preset: Option<LayoutPreset>) -> bool {
        self.in_active_window(|win| win.select_layout(preset))
    }

    /// Spreads the active window's panes evenly, keeping their arrangement.
    pub fn spread_layout(&mut self) -> bool {
        self.in_active_window(Window::spread_layout)
    }

    /// Zooms (`on`), unzooms, or toggles (`None`) the active window. False when there is none.
    pub fn set_zoom(&mut self, on: Option<bool>) -> bool {
        self.in_active_window(|win| win.zoomed = on.unwrap_or(!win.zoomed))
    }

    /// Unzooms the active window.
    pub fn unzoom(&mut self) {
        self.in_active_window(|win| win.zoomed = false);
    }

    /// Whether the window holding `target` (the active window when `None`) is zoomed.
    pub fn zoomed_at(&self, target: Option<&str>) -> bool {
        let pos = target
            .and_then(|t| self.window_of(t))
            .unwrap_or(self.active_idx);
        self.window_at(pos).is_some_and(|w| w.zoomed)
    }

    /// Zooms the window holding pane `pane`.
    pub fn zoom_window_of(&mut self, pane: &str) {
        if let Some(win) = self.window_of(pane).and_then(|pos| self.window_at_mut(pos)) {
            win.zoomed = true;
        }
    }

    /// Rotates the active window's panes through its layout slots; false when there is no window.
    pub fn rotate_active(&mut self, down: bool, keep_zoom: bool) -> bool {
        self.in_active_window(|win| {
            win.rotate(down);
            if !keep_zoom {
                win.zoomed = false;
            }
        })
    }

    pub fn resize_split(
        &mut self,
        name: &str,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    ) -> bool {
        self.window_of(name)
            .and_then(|pos| self.window_at_mut(pos))
            .is_some_and(|win| win.resize_split(name, side, cells, Rect::new(0, 0, width, height)))
    }

    /// The size `resize-window` pinned the window at index `number` to (the active window when
    /// `None`), if it has one.
    pub fn manual_size_at(&self, number: Option<usize>) -> Option<(u16, u16)> {
        let win = match number {
            Some(n) => self.window_numbered(n)?,
            None => self.active_window()?,
        };
        win.manual_size
    }

    /// Pins the size of the window at index `number` (the active one when `None`); `None` clears the
    /// pin.
    pub fn set_manual_size(
        &mut self,
        number: Option<usize>,
        size: Option<(u16, u16)>,
    ) -> Result<(), String> {
        self.window_mut_at(number)?.manual_size = size;
        Ok(())
    }

    pub fn compute_layout(&self, width: u16, height: u16) -> Vec<(String, Rect, bool)> {
        self.compute_layout_with_dividers(width, height).0
    }

    pub fn compute_layout_with_dividers(
        &self,
        width: u16,
        height: u16,
    ) -> (Vec<(String, Rect, bool)>, Vec<Divider>) {
        let bounds = Rect::new(0, 0, width, height);
        match self.active_window() {
            Some(win) => win.compute_layout(bounds),
            None => (Vec::new(), Vec::new()),
        }
    }
}
