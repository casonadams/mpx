use super::Workspace;

impl Workspace {
    /// Makes the link at position `pos` the active one. Leaving a window unzooms it; the alerts of
    /// the index are cleared.
    pub(super) fn set_active(&mut self, pos: usize) {
        if pos >= self.links.len() {
            return;
        }
        if pos != self.active_idx {
            let leaving = self.window_at(self.active_idx).map(|w| w.id);
            if leaving != self.window_at(pos).map(|w| w.id)
                && let Some(left) = self.window_at_mut(self.active_idx)
            {
                left.zoomed = false;
            }
            self.last_window = Some(self.active_idx);
            self.active_idx = pos;
        }
        let link = &mut self.links[pos];
        link.has_activity = false;
        link.has_bell = false;
        link.has_silence = false;
    }

    pub fn next_window(&mut self) {
        if !self.links.is_empty() {
            self.set_active((self.active_idx + 1) % self.links.len());
        }
    }

    pub fn prev_window(&mut self) {
        if !self.links.is_empty() {
            let len = self.links.len();
            self.set_active((self.active_idx + len - 1) % len);
        }
    }

    pub fn select_window(&mut self, number: usize) -> bool {
        match self.position_of(number) {
            Some(idx) => {
                self.set_active(idx);
                true
            }
            None => false,
        }
    }

    /// Switches to the index that was active before the current one.
    pub fn last_window_switch(&mut self) -> bool {
        match self.last_window {
            Some(idx) if idx < self.links.len() => {
                self.set_active(idx);
                true
            }
            _ => false,
        }
    }

    /// Makes the index `number` the one active before the current one (`None` for none).
    pub fn set_last_window(&mut self, number: Option<usize>) {
        self.last_window = number.and_then(|n| self.position_of(n));
    }

    pub fn focus_next(&mut self) {
        self.in_active_window(|win| {
            win.focus_next();
            win.zoomed = false;
        });
    }

    pub fn focus_last(&mut self) -> bool {
        self.focus_last_with(false)
    }

    /// Focuses the last pane of the active window. Unless `keep_zoom`, a move unzooms the window.
    pub fn focus_last_with(&mut self, keep_zoom: bool) -> bool {
        let Some(win) = self.active_window_mut() else {
            return false;
        };
        let moved = win.focus_last();
        if moved && !keep_zoom {
            win.zoomed = false;
        }
        moved
    }

    /// Focuses a pane, switching to its window. Focusing unzooms that window.
    pub fn focus(&mut self, name: &str) -> bool {
        self.focus_with(name, false)
    }

    /// Focuses a pane, switching to its window. Unless `keep_zoom`, focusing unzooms that window.
    pub fn focus_with(&mut self, name: &str, keep_zoom: bool) -> bool {
        let Some(pos) = self.window_of(name) else {
            return false;
        };
        if let Some(win) = self.window_at_mut(pos) {
            win.set_focus(name);
            if !keep_zoom {
                win.zoomed = false;
            }
        }
        self.set_active(pos);
        true
    }
}
