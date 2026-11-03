use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use super::tree::{Direction, Divider, LayoutNode, preset_tree};
use crate::protocol::{LayoutPreset, Side};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowTab {
    pub index: usize,
    pub name: String,
    pub is_active: bool,
    #[serde(default)]
    pub is_last: bool,
    #[serde(default)]
    pub has_activity: bool,
    #[serde(default)]
    pub has_bell: bool,
    #[serde(default)]
    pub has_silence: bool,
    /// The window's panes all receive typed input (tmux `synchronize-panes`).
    #[serde(default)]
    pub synchronized: bool,
    #[serde(default)]
    pub has_marked: bool,
    #[serde(default)]
    pub marked_pane: Option<String>,
    #[serde(default)]
    pub agent_badge: Option<String>,
    /// The window is zoomed: its focused pane fills the window's area (tmux `Z` flag).
    #[serde(default)]
    pub zoomed: bool,
    /// The window's active pane (tmux `pane_active`), whether or not the window is shown.
    #[serde(default)]
    pub focused: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: usize,
    /// The number `select-window` and friends address. Numbers are unique and ascending; with
    /// renumber-windows on they are the positions.
    pub number: usize,
    pub name: String,
    /// Name given at creation; automatic naming falls back to it.
    #[serde(default)]
    pub label: String,
    /// Set by an explicit rename; disables automatic naming.
    #[serde(default)]
    pub renamed: bool,
    pub root: Option<LayoutNode>,
    pub focused: Option<String>,
    #[serde(default)]
    pub last_focused: Option<String>,
    /// Preset most recently applied by `select-layout`, used to cycle.
    #[serde(default)]
    pub preset: Option<LayoutPreset>,
    #[serde(default)]
    pub has_activity: bool,
    #[serde(default)]
    pub has_bell: bool,
    /// The window has been quiet for `monitor-silence` seconds while in the background (tmux `~`).
    #[serde(default)]
    pub has_silence: bool,
    /// The window's panes all receive typed input (tmux `synchronize-panes`).
    #[serde(default)]
    pub synchronized: bool,
    /// When the window last printed output; `None` until it does. Not persisted.
    #[serde(skip)]
    pub last_output: Option<Instant>,
    /// Zoomed: only the focused pane is shown. Any change to the layout unzooms the window.
    #[serde(default)]
    pub zoomed: bool,
}

impl Window {
    pub fn new(id: usize, number: usize, name: String, root_node: String) -> Self {
        Self {
            id,
            number,
            label: name.clone(),
            name,
            renamed: false,
            root: Some(LayoutNode::leaf(root_node.clone())),
            focused: Some(root_node),
            last_focused: None,
            preset: None,
            has_activity: false,
            has_bell: false,
            has_silence: false,
            synchronized: false,
            last_output: None,
            zoomed: false,
        }
    }

    pub fn leaves(&self) -> Vec<String> {
        self.root
            .as_ref()
            .map(LayoutNode::all_leaves)
            .unwrap_or_default()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.root.as_ref().is_some_and(|r| r.has_leaf(name))
    }

    /// Splits `target` (or the focused pane, or the first pane). `percent` is the share the new
    /// pane takes; the new pane is placed right of / below the target.
    pub fn split_at(
        &mut self,
        target: Option<&str>,
        node_name: String,
        dir: Direction,
        percent: Option<u8>,
        focus: bool,
    ) -> bool {
        let Some(root) = self.root.as_mut() else {
            self.root = Some(LayoutNode::leaf(node_name.clone()));
            self.focused = Some(node_name);
            return true;
        };
        let leaves = root.all_leaves();
        let anchor = target
            .map(str::to_string)
            .or_else(|| self.focused.clone())
            .or_else(|| leaves.first().cloned());
        let Some(anchor) = anchor else { return false };
        let ratio = percent.map_or(0.5, |p| 1.0 - f32::from(p.clamp(1, 99)) / 100.0);
        if !root.split_leaf_with(&anchor, node_name.clone(), dir, ratio) {
            return false;
        }
        self.zoomed = false;
        if focus {
            self.set_focus(&node_name);
        }
        true
    }

    pub fn remove(&mut self, node_name: &str) {
        if self.contains(node_name) {
            self.zoomed = false;
        }
        if let Some(root) = self.root.take() {
            self.root = root.remove_leaf(node_name);
        }
        if self.last_focused.as_deref() == Some(node_name) {
            self.last_focused = None;
        }
        if self.focused.as_deref() == Some(node_name) {
            self.focused = self.leaves().into_iter().next();
        }
    }

    /// Focuses a leaf; the previously focused pane becomes the `last_focused` pane.
    pub fn set_focus(&mut self, name: &str) -> bool {
        if !self.contains(name) {
            return false;
        }
        if self.focused.as_deref() != Some(name) {
            self.last_focused = self.focused.take();
            self.focused = Some(name.to_string());
        }
        true
    }

    pub fn focus_last(&mut self) -> bool {
        match self.last_focused.clone() {
            Some(last) if self.contains(&last) => {
                self.last_focused = self.focused.take();
                self.focused = Some(last);
                true
            }
            _ => false,
        }
    }

    pub fn focus_next(&mut self) {
        let leaves = self.leaves();
        if leaves.is_empty() {
            self.focused = None;
            return;
        }
        let current = self
            .focused
            .as_ref()
            .and_then(|f| leaves.iter().position(|l| l == f))
            .unwrap_or(0);
        let name = leaves[(current + 1) % leaves.len()].clone();
        self.set_focus(&name);
    }

    pub fn focus(&mut self, name: &str) {
        self.set_focus(name);
    }

    /// Applies `preset` (or the one after the current preset when `None`) to the leaves.
    pub fn select_layout(&mut self, preset: Option<LayoutPreset>) {
        self.zoomed = false;
        let next = preset.unwrap_or_else(|| next_preset(self.preset));
        if let Some(root) = preset_tree(next, &self.leaves()) {
            self.root = Some(root);
            self.preset = Some(next);
        }
    }

    /// Spreads the panes evenly without rearranging them.
    pub fn spread_layout(&mut self) {
        self.zoomed = false;
        self.root.iter_mut().for_each(LayoutNode::spread);
    }

    /// Moves every pane to the slot before it, or after it when `down`: the pane names turn
    /// through the fixed layout slots, so geometry stays and focus stays on the same pane.
    pub fn rotate(&mut self, down: bool) {
        let Some(root) = self.root.as_mut() else {
            return;
        };
        let mut names = root.all_leaves();
        if down {
            names.rotate_right(1);
        } else {
            names.rotate_left(1);
        }
        let mut names = names.into_iter();
        root.map_leaves(&mut |slot| {
            if let Some(name) = names.next() {
                *slot = name;
            }
        });
    }

    pub fn resize_split(&mut self, name: &str, side: Side, cells: u16, bounds: Rect) -> bool {
        match self.root.as_mut() {
            Some(root) => root.resize_border(name, side, cells, bounds),
            None => false,
        }
    }

    pub fn compute_layout(&self, bounds: Rect) -> (Vec<(String, Rect, bool)>, Vec<Divider>) {
        let (rects, dividers) = match &self.root {
            Some(r) => r.compute_layout(bounds),
            None => return (Vec::new(), Vec::new()),
        };

        let panes = rects
            .into_iter()
            .map(|(name, rect)| {
                let is_focused = self.focused.as_deref() == Some(&name);
                (name, rect, is_focused)
            })
            .collect();
        (panes, dividers)
    }
}

fn next_preset(current: Option<LayoutPreset>) -> LayoutPreset {
    let all = LayoutPreset::ALL;
    match current.and_then(|c| all.iter().position(|p| *p == c)) {
        Some(i) => all[(i + 1) % all.len()],
        None => all[0],
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub windows: Vec<Window>,
    pub active_idx: usize,
    pub next_window_id: usize,
    #[serde(default)]
    pub last_window: Option<usize>,
    #[serde(default)]
    pub marked_pane: Option<String>,
    /// Whether windows created from now on start with `synchronize-panes` on (config `synchronize_panes`).
    #[serde(skip)]
    synchronize_new: bool,
    /// Whether closing a window renumbers the windows after it (tmux `renumber-windows`).
    #[serde(skip)]
    renumber_windows: bool,
}

impl Workspace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            windows: Vec::new(),
            active_idx: 0,
            next_window_id: 0,
            last_window: None,
            marked_pane: None,
            renumber_windows: true,
            synchronize_new: false,
        }
    }

    /// Whether windows created from now on start with `synchronize-panes` on.
    pub fn set_synchronize_new(&mut self, on: bool) {
        self.synchronize_new = on;
    }

    /// Whether closing a window renumbers the windows after it (tmux `renumber-windows`).
    pub fn set_renumber_windows(&mut self, on: bool) {
        self.renumber_windows = on;
    }

    pub fn new_window(&mut self, window_name: String, root_node: String) -> usize {
        self.new_window_with_focus(window_name, root_node, true)
    }

    /// Adds a window at its number's place in the order, numbered with the lowest free number, and
    /// returns that number.
    pub fn new_window_with_focus(
        &mut self,
        window_name: String,
        root_node: String,
        focus: bool,
    ) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        let number = self.lowest_free_number();
        let pos = self.insert_window(Window::new(id, number, window_name, root_node));
        if focus {
            self.set_active(pos);
        }
        number
    }

    /// Adds a window with a restored layout, numbered `number` unless renumber-windows is on or that
    /// number is taken, then the lowest free one. `sticky` keeps a user-chosen name; otherwise `label`
    /// names it until automatic naming takes over. Focus falls back to the first pane. Returns the
    /// window's number.
    pub fn install_window(
        &mut self,
        number: usize,
        label: String,
        sticky: Option<String>,
        root: LayoutNode,
        focused: Option<String>,
    ) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        let focused = focused
            .filter(|f| root.has_leaf(f))
            .or_else(|| root.all_leaves().into_iter().next());
        let number = if self.renumber_windows || self.position_of(number).is_some() {
            self.lowest_free_number()
        } else {
            number
        };
        let pos = self.insert_window(Window {
            id,
            number,
            name: sticky.clone().unwrap_or_else(|| label.clone()),
            label,
            renamed: sticky.is_some(),
            root: Some(root),
            focused,
            last_focused: None,
            preset: None,
            has_activity: false,
            has_bell: false,
            has_silence: false,
            synchronized: false,
            last_output: None,
            zoomed: false,
        });
        self.windows[pos].number
    }

    fn set_active(&mut self, idx: usize) {
        if idx < self.windows.len() {
            if idx != self.active_idx {
                if let Some(left) = self.windows.get_mut(self.active_idx) {
                    left.zoomed = false;
                }
                self.last_window = Some(self.active_idx);
                self.active_idx = idx;
            }
            if let Some(w) = self.windows.get_mut(idx) {
                w.has_activity = false;
                w.has_bell = false;
                w.has_silence = false;
            }
        }
    }

    pub fn next_window(&mut self) {
        if !self.windows.is_empty() {
            self.set_active((self.active_idx + 1) % self.windows.len());
        }
    }

    pub fn prev_window(&mut self) {
        if !self.windows.is_empty() {
            let prev = if self.active_idx == 0 {
                self.windows.len() - 1
            } else {
                self.active_idx - 1
            };
            self.set_active(prev);
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

    /// Switches to the window that was active before the current one.
    pub fn last_window_switch(&mut self) -> bool {
        match self.last_window {
            Some(idx) if idx < self.windows.len() => {
                self.set_active(idx);
                true
            }
            _ => false,
        }
    }

    pub fn window_tabs(&self) -> Vec<WindowTab> {
        self.windows
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let marked_pane = self.marked_pane.as_ref().filter(|m| w.contains(m)).cloned();
                let has_marked = marked_pane.is_some();
                WindowTab {
                    index: w.number,
                    name: w.name.clone(),
                    is_active: i == self.active_idx,
                    is_last: Some(i) == self.last_window,
                    has_activity: w.has_activity,
                    has_bell: w.has_bell,
                    has_silence: w.has_silence,
                    synchronized: w.synchronized,
                    zoomed: w.zoomed,
                    focused: w.focused.clone(),
                    has_marked,
                    marked_pane,
                    agent_badge: None,
                }
            })
            .collect()
    }

    fn mark_window_flag(
        &mut self,
        pane_name: &str,
        flag: impl FnOnce(&mut Window) -> &mut bool,
    ) -> bool {
        let Some(w_idx) = self.window_of(pane_name) else {
            return false;
        };
        if w_idx == self.active_idx {
            return false;
        }
        let Some(w) = self.windows.get_mut(w_idx) else {
            return false;
        };
        let target = flag(w);
        if !*target {
            *target = true;
            return true;
        }
        false
    }

    pub fn mark_activity(&mut self, pane_name: &str) -> bool {
        self.mark_window_flag(pane_name, |w| &mut w.has_activity)
    }

    pub fn mark_bell(&mut self, pane_name: &str) -> bool {
        self.mark_window_flag(pane_name, |w| &mut w.has_bell)
    }

    /// Records output from a pane: its window's silence timer restarts and its silence flag clears.
    pub fn note_output(&mut self, pane_name: &str, now: Instant) {
        if let Some(w) = self
            .window_of(pane_name)
            .and_then(|i| self.windows.get_mut(i))
        {
            w.last_output = Some(now);
            w.has_silence = false;
        }
    }

    /// Flags background windows that have been quiet for `secs` seconds (`monitor-silence`).
    /// Returns whether any flag changed.
    pub fn expire_silence(&mut self, secs: u64, now: Instant) -> bool {
        let active = self.active_idx;
        let quiet = Duration::from_secs(secs);
        let mut changed = false;
        for (i, w) in self.windows.iter_mut().enumerate() {
            if i != active
                && !w.has_silence
                && w.last_output
                    .is_some_and(|t| now.duration_since(t) >= quiet)
            {
                w.has_silence = true;
                changed = true;
            }
        }
        changed
    }

    /// When the next background window goes quiet for `secs` seconds, if any is still counting.
    pub fn next_silence(&self, secs: u64) -> Option<Instant> {
        let quiet = Duration::from_secs(secs);
        self.windows
            .iter()
            .enumerate()
            .filter(|(i, w)| *i != self.active_idx && !w.has_silence)
            .filter_map(|(_, w)| w.last_output)
            .map(|t| t + quiet)
            .min()
    }

    /// Turns `synchronize-panes` on or off for the window numbered `number` (the active window when
    /// `None`); `on` None toggles it.
    pub fn synchronize(&mut self, number: Option<usize>, on: Option<bool>) -> Result<(), String> {
        let idx = match number {
            Some(n) => self.position_of(n).ok_or_else(|| window_not_found(n))?,
            None => self.active_idx,
        };
        let win = self
            .windows
            .get_mut(idx)
            .ok_or_else(|| "No active window".to_string())?;
        win.synchronized = on.unwrap_or(!win.synchronized);
        Ok(())
    }

    pub fn active_window(&self) -> Option<&Window> {
        self.windows.get(self.active_idx)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(self.active_idx)
    }

    pub fn window_of(&self, name: &str) -> Option<usize> {
        self.windows.iter().position(|w| w.contains(name))
    }

    /// The number of the window holding pane `name`.
    pub fn number_of(&self, name: &str) -> Option<usize> {
        self.window_of(name).map(|idx| self.windows[idx].number)
    }

    /// The number of the active window, if there is one.
    pub fn active_number(&self) -> Option<usize> {
        self.active_window().map(|w| w.number)
    }

    /// Number of panes across all windows.
    pub fn pane_count(&self) -> usize {
        self.windows.iter().map(|w| w.leaves().len()).sum()
    }
    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        self.add_or_split_with_focus(node_name, dir, true);
    }

    pub fn add_or_split_with_focus(&mut self, node_name: String, dir: Direction, focus: bool) {
        if self.windows.is_empty() {
            let win_name = node_name.clone();
            self.new_window_with_focus(win_name, node_name, focus);
            return;
        }

        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.split_at(None, node_name, dir, None, focus);
        }
    }

    /// Splits `target` (default: the focused pane of the active window). The new pane's window
    /// becomes active when `focus` is set.
    pub fn split_pane(
        &mut self,
        target: Option<&str>,
        node_name: String,
        dir: Direction,
        percent: Option<u8>,
        focus: bool,
    ) -> Result<(), String> {
        let idx = match target {
            Some(t) => self
                .window_of(t)
                .ok_or_else(|| format!("Node '{}' not found", t))?,
            None if self.windows.is_empty() => {
                self.new_window_with_focus(node_name.clone(), node_name, focus);
                return Ok(());
            }
            None => self.active_idx,
        };
        if !self.windows[idx].split_at(target, node_name, dir, percent, focus) {
            return Err("Split target not found".to_string());
        }
        if focus {
            self.set_active(idx);
        }
        Ok(())
    }

    pub fn remove(&mut self, node_name: &str) {
        if self.marked_pane.as_deref() == Some(node_name) {
            self.marked_pane = None;
        }
        for win in &mut self.windows {
            win.remove(node_name);
        }
        self.drop_empty_windows();
    }

    fn drop_empty_windows(&mut self) {
        let active = self.windows.get(self.active_idx).map(|w| w.id);
        let last = self
            .last_window
            .and_then(|i| self.windows.get(i))
            .map(|w| w.id);
        self.windows.retain(|w| w.root.is_some());
        self.active_idx = active
            .and_then(|id| self.windows.iter().position(|w| w.id == id))
            .unwrap_or_else(|| self.windows.len().saturating_sub(1));
        self.last_window = last.and_then(|id| self.windows.iter().position(|w| w.id == id));
        if self.renumber_windows {
            self.compact_numbers();
        }
    }

    pub fn focus_next(&mut self) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.focus_next();
            win.zoomed = false;
        }
    }

    pub fn focus_last(&mut self) -> bool {
        self.focus_last_with(false)
    }

    /// Focuses the last pane of the active window. Unless `keep_zoom`, a move unzooms the window.
    pub fn focus_last_with(&mut self, keep_zoom: bool) -> bool {
        let Some(win) = self.windows.get_mut(self.active_idx) else {
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
        match self.window_of(name) {
            Some(idx) => {
                self.windows[idx].set_focus(name);
                if !keep_zoom {
                    self.windows[idx].zoomed = false;
                }
                self.set_active(idx);
                true
            }
            None => false,
        }
    }

    pub fn rename_window(&mut self, number: usize, name: String) -> bool {
        match self.position_of(number) {
            Some(idx) => {
                let win = &mut self.windows[idx];
                win.name = name;
                win.renamed = true;
                true
            }
            None => false,
        }
    }

    /// Sets the automatic name of the window numbered `number` unless it was renamed explicitly.
    pub fn set_auto_name(&mut self, number: usize, name: Option<String>) {
        if let Some(idx) = self.position_of(number) {
            let win = &mut self.windows[idx];
            if !win.renamed {
                win.name = name.unwrap_or_else(|| win.label.clone());
            }
        }
    }

    /// Removes the window numbered `number` and returns its panes. With renumber-windows on, the
    /// windows after it are renumbered.
    pub fn kill_window(&mut self, number: usize) -> Option<Vec<String>> {
        let idx = self.position_of(number)?;
        let panes = self.windows[idx].leaves();
        if let Some(marked) = &self.marked_pane
            && panes.iter().any(|p| p == marked)
        {
            self.marked_pane = None;
        }
        self.windows.remove(idx);
        let shift = |i: usize| if i > idx { i - 1 } else { i };
        self.active_idx = shift(self.active_idx).min(self.windows.len().saturating_sub(1));
        self.last_window = self.last_window.filter(|&l| l != idx).map(shift);
        if self.renumber_windows {
            self.compact_numbers();
        }
        Some(panes)
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
            for slot in [&mut win.focused, &mut win.last_focused] {
                if let Some(n) = slot.as_mut() {
                    swap_name(n);
                }
            }
        }
        true
    }

    /// Moves a pane out of its window into a new window of its own.
    pub fn break_pane(&mut self, name: &str) -> bool {
        let Some(idx) = self.window_of(name) else {
            return false;
        };
        if self.windows[idx].leaves().len() <= 1 {
            return false;
        }
        self.windows[idx].remove(name);
        let id = self.next_window_id;
        self.next_window_id += 1;
        let number = self.lowest_free_number();
        let pos = self.insert_window(Window::new(id, number, name.to_string(), name.to_string()));
        self.set_active(pos);
        true
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
        self.windows[from].remove(src);
        let to = self.window_of(target).unwrap_or(from);
        if !self.windows[to].split_at(Some(target), src.to_string(), dir, None, !detach) {
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

    /// Exchanges the numbers of two windows (tmux `swap-window`); the active and last window stay the
    /// same windows.
    pub fn swap_windows(&mut self, a: usize, b: usize) -> Result<(), String> {
        let pa = self.position_of(a).ok_or_else(|| window_not_found(a))?;
        let pb = self.position_of(b).ok_or_else(|| window_not_found(b))?;
        if pa != pb {
            self.reorder(|windows| {
                let number = windows[pa].number;
                windows[pa].number = windows[pb].number;
                windows[pb].number = number;
                windows.sort_by_key(|w| w.number);
            });
        }
        Ok(())
    }

    /// Moves window `from` to number `to` (tmux `move-window`). With renumber-windows on, `to` is a
    /// position and the windows are renumbered afterwards; otherwise `to` must be free.
    pub fn move_window(&mut self, from: usize, to: usize) -> Result<(), String> {
        let from_pos = self
            .position_of(from)
            .ok_or_else(|| window_not_found(from))?;
        if self.renumber_windows {
            if to >= self.windows.len() {
                return Err(window_not_found(to));
            }
            self.reorder(|windows| {
                let window = windows.remove(from_pos);
                windows.insert(to, window);
            });
            self.compact_numbers();
        } else if from != to {
            if self.position_of(to).is_some() {
                return Err(format!("Window {to} already exists"));
            }
            self.reorder(|windows| {
                windows[from_pos].number = to;
                windows.sort_by_key(|w| w.number);
            });
        }
        Ok(())
    }

    fn reorder(&mut self, change: impl FnOnce(&mut Vec<Window>)) {
        let active = self.windows.get(self.active_idx).map(|w| w.id);
        let last = self
            .last_window
            .and_then(|i| self.windows.get(i))
            .map(|w| w.id);
        change(&mut self.windows);
        let position =
            |id: Option<usize>| id.and_then(|id| self.windows.iter().position(|w| w.id == id));
        self.active_idx = position(active).unwrap_or(0);
        self.last_window = position(last);
    }

    fn position_of(&self, number: usize) -> Option<usize> {
        self.windows
            .binary_search_by_key(&number, |w| w.number)
            .ok()
    }

    /// The lowest number no window has: the first gap in the ascending numbers.
    fn lowest_free_number(&self) -> usize {
        self.windows
            .iter()
            .enumerate()
            .find(|(i, w)| w.number != *i)
            .map_or(self.windows.len(), |(i, _)| i)
    }

    /// Inserts a window in number order and returns its position. The active and last windows stay
    /// the same windows.
    fn insert_window(&mut self, mut window: Window) -> usize {
        window.synchronized = self.synchronize_new;
        let pos = self.windows.partition_point(|w| w.number < window.number);
        self.reorder(|windows| windows.insert(pos, window));
        pos
    }

    /// Numbers the windows 0, 1, ... in order (tmux `move-window -r`).
    pub fn compact_numbers(&mut self) {
        for (i, win) in self.windows.iter_mut().enumerate() {
            win.number = i;
        }
    }

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
        let idx = target.and_then(|t| self.window_of(t)).unwrap_or(self.active_idx);
        self.windows.get(idx).is_some_and(|w| w.zoomed)
    }

    /// Zooms the window holding pane `pane`.
    pub fn zoom_window_of(&mut self, pane: &str) {
        if let Some(idx) = self.window_of(pane) {
            self.windows[idx].zoomed = true;
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

    /// Runs `change` on the active window; false when there is none.
    fn in_active_window(&mut self, change: impl FnOnce(&mut Window)) -> bool {
        match self.active_window_mut() {
            Some(win) => {
                change(win);
                true
            }
            None => false,
        }
    }

    pub fn resize_split(
        &mut self,
        name: &str,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    ) -> bool {
        let Some(idx) = self.window_of(name) else {
            return false;
        };
        self.windows[idx].resize_split(name, side, cells, Rect::new(0, 0, width, height))
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

/// The error for a window number that no window has.
pub fn window_not_found(number: usize) -> String {
    format!("No window {number}")
}
