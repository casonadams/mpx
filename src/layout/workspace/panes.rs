use super::{Window, Workspace};
use crate::layout::tree::Direction;

impl Workspace {
    /// Splits `target` (default: the focused pane of the active window). The new pane's index
    /// becomes active when `focus` is set.
    pub fn split_pane(
        &mut self,
        target: Option<&str>,
        node_name: String,
        dir: Direction,
        percent: Option<u8>,
        focus: bool,
    ) -> Result<(), String> {
        let pos = match target {
            Some(t) => self
                .window_of(t)
                .ok_or_else(|| format!("Node '{}' not found", t))?,
            None if self.links.is_empty() => {
                self.new_window_with_focus(node_name.clone(), node_name, focus);
                return Ok(());
            }
            None => self.active_idx,
        };
        let split = self
            .window_at_mut(pos)
            .is_some_and(|w| w.split_at(target, node_name, dir, percent, focus));
        if !split {
            return Err("Split target not found".to_string());
        }
        if focus {
            self.set_active(pos);
        }
        Ok(())
    }

    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        self.add_or_split_with_focus(node_name, dir, true);
    }

    pub fn add_or_split_with_focus(&mut self, node_name: String, dir: Direction, focus: bool) {
        if self.links.is_empty() {
            self.new_window_with_focus(node_name.clone(), node_name, focus);
            return;
        }
        if let Some(win) = self.active_window_mut() {
            win.split_at(None, node_name, dir, None, focus);
        }
    }

    /// Removes pane `node_name` from its window; a window left without panes goes away.
    pub fn remove(&mut self, node_name: &str) {
        self.forget_panes(|p| p == node_name);
        for win in &mut self.windows {
            win.remove(node_name);
        }
        self.drop_empty_windows();
    }

    /// Drops the marks and input-off flags of the panes `gone` selects.
    pub(super) fn forget_panes(&mut self, gone: impl Fn(&str) -> bool) {
        if self.marked_pane.as_deref().is_some_and(&gone) {
            self.marked_pane = None;
        }
        self.input_off.retain(|p| !gone(p));
    }

    /// Moves pane `src` next to `target`, splitting `target` in direction `dir`, whichever windows
    /// they live in. A window left without panes goes away.
    pub fn join_pane(
        &mut self,
        src: &str,
        target: &str,
        dir: Direction,
        detach: bool,
    ) -> Result<(), String> {
        if src == target {
            return Err("Cannot join a pane to itself".to_string());
        }
        let from = self
            .window_of(src)
            .ok_or_else(|| format!("Node '{src}' not found"))?;
        if self.window_of(target).is_none() {
            return Err(format!("Node '{target}' not found"));
        }
        let previous = self.active_window().and_then(|w| w.focused.clone());
        if let Some(win) = self.window_at_mut(from) {
            win.remove(src);
        }
        let to = self.window_of(target).unwrap_or(from);
        let joined = self
            .window_at_mut(to)
            .is_some_and(|w| w.split_at(Some(target), src.to_string(), dir, None, !detach));
        if !joined {
            return Err("Split target not found".to_string());
        }
        self.drop_empty_windows();
        if !detach {
            if let Some(idx) = self.window_of(target) {
                self.set_active(idx);
            }
        } else if let Some(previous) = previous.filter(|p| self.window_of(p).is_some()) {
            self.focus_with(&previous, true);
        }
        Ok(())
    }

    /// Moves a pane out of its window into a new window of its own.
    pub fn break_pane(&mut self, name: &str) -> bool {
        let Some(pos) = self.window_of(name) else {
            return false;
        };
        let Some(win) = self.window_at_mut(pos) else {
            return false;
        };
        if win.leaves().len() <= 1 {
            return false;
        }
        win.remove(name);
        let id = self.push_window(|id| Window::new(id, name.to_string(), name.to_string()));
        let number = self.lowest_free_number();
        let pos = self.insert_link(id, number);
        self.set_active(pos);
        true
    }

    /// Exchanges two panes by name, wherever they live.
    pub fn swap(&mut self, a: &str, b: &str) -> bool {
        if a == b || self.window_of(a).is_none() || self.window_of(b).is_none() {
            return false;
        }
        let mut swap_name = |n: &mut String| {
            if n.as_str() == a {
                *n = b.to_string();
            } else if n.as_str() == b {
                *n = a.to_string();
            }
        };
        for win in &mut self.windows {
            if let Some(root) = win.root.as_mut() {
                root.map_leaves(&mut swap_name);
            }
            if let Some(n) = win.focused.as_mut() {
                swap_name(n);
            }
            win.focus_history.iter_mut().for_each(&mut swap_name);
        }
        true
    }

    /// Number of panes across all windows.
    pub fn pane_count(&self) -> usize {
        self.windows.iter().map(|w| w.leaves().len()).sum()
    }

    /// Toggles the marked flag on `pane`: if already marked, clears the mark;
    /// otherwise marks it. Returns `true` if the pane is now marked.
    pub fn mark_pane(&mut self, pane: &str) -> bool {
        if self.marked_pane.as_deref() == Some(pane) {
            self.marked_pane = None;
            false
        } else if self.window_of(pane).is_some() {
            self.marked_pane = Some(pane.to_string());
            true
        } else {
            false
        }
    }

    /// Clears the marked pane unconditionally.
    pub fn clear_mark_pane(&mut self) {
        self.marked_pane = None;
    }

    /// The currently marked pane, if any.
    pub fn marked_pane(&self) -> Option<&str> {
        self.marked_pane.as_deref()
    }

    /// Turns input to `pane` off or on (tmux `select-pane -d`/`-e`).
    pub fn set_input_off(&mut self, pane: &str, off: bool) {
        if off {
            self.input_off.insert(pane.to_string());
        } else {
            self.input_off.remove(pane);
        }
    }

    /// Whether input to `pane` is off: keys sent to it are dropped.
    pub fn input_off(&self, pane: &str) -> bool {
        self.input_off.contains(pane)
    }
}
