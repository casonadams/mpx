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
    /// Every index the window is linked at, this one included, in number order (tmux
    /// `window_linked`).
    #[serde(default)]
    pub linked: Vec<usize>,
}

/// A window: its panes, split tree, focus and name. Every index it is linked at shows this one
/// window, so a pane killed here is gone from all of them (tmux `struct window`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: usize,
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
    /// The window's panes all receive typed input (tmux `synchronize-panes`).
    #[serde(default)]
    pub synchronized: bool,
    /// Zoomed: only the focused pane is shown. Any change to the layout unzooms the window.
    #[serde(default)]
    pub zoomed: bool,
    /// The size `resize-window` set, which replaces the size the clients give (tmux `window-size
    /// manual`). Not persisted: a restored window follows its clients again.
    #[serde(skip)]
    pub manual_size: Option<(u16, u16)>,
}

impl Window {
    pub fn new(id: usize, name: String, root_node: String) -> Self {
        Self {
            id,
            label: name.clone(),
            name,
            renamed: false,
            root: Some(LayoutNode::leaf(root_node.clone())),
            focused: Some(root_node),
            last_focused: None,
            preset: None,
            synchronized: false,
            zoomed: false,
            manual_size: None,
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

/// One index of the session (tmux winlink): the window shown there, and the alerts raised there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Identifies the index across renumbering, so a change can tell which links were added or
    /// removed.
    pub id: usize,
    /// The number `select-window` and friends address. Numbers are unique and ascending; with
    /// renumber-windows on they are the positions.
    pub number: usize,
    /// Id of the window shown at this index.
    pub window: usize,
    #[serde(default)]
    pub has_activity: bool,
    #[serde(default)]
    pub has_bell: bool,
    /// The window has been quiet for `monitor-silence` seconds while in the background (tmux `~`).
    #[serde(default)]
    pub has_silence: bool,
    /// When the window last printed output; `None` until it does. Not persisted.
    #[serde(skip)]
    pub last_output: Option<Instant>,
}

impl Link {
    fn new(id: usize, number: usize, window: usize) -> Self {
        Self {
            id,
            number,
            window,
            has_activity: false,
            has_bell: false,
            has_silence: false,
            last_output: None,
        }
    }
}

/// Where `link-window` puts a new index and how it treats the one it is given (tmux `link-window`
/// flags `-a`, `-b`, `-k` and `-d`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkPlace {
    /// Insert after the given index (`-a`).
    pub after: bool,
    /// Insert before the given index (`-b`).
    pub before: bool,
    /// Replace the window at an occupied index, unlinking it (`-k`).
    pub kill: bool,
    /// Keep the active index as it is (`-d`).
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// Every window, once each, however many indices show it.
    pub windows: Vec<Window>,
    /// The session's indices, in number order.
    pub links: Vec<Link>,
    /// Position in `links` of the active index.
    pub active_idx: usize,
    pub next_window_id: usize,
    pub next_link_id: usize,
    /// Position in `links` of the index that was active before the current one.
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
            links: Vec::new(),
            active_idx: 0,
            next_window_id: 0,
            next_link_id: 0,
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

    /// Adds a window at the lowest free number and returns that number.
    pub fn new_window_with_focus(
        &mut self,
        window_name: String,
        root_node: String,
        focus: bool,
    ) -> usize {
        let number = self.lowest_free_number();
        let id = self.push_window(|id| Window::new(id, window_name, root_node));
        let pos = self.insert_link(id, number);
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
        let focused = focused
            .filter(|f| root.has_leaf(f))
            .or_else(|| root.all_leaves().into_iter().next());
        let number = self.number_for(number);
        let id = self.push_window(|id| Window {
            id,
            name: sticky.clone().unwrap_or_else(|| label.clone()),
            renamed: sticky.is_some(),
            label,
            root: Some(root),
            focused,
            last_focused: None,
            preset: None,
            synchronized: false,
            zoomed: false,
            manual_size: None,
        });
        let pos = self.insert_link(id, number);
        self.links[pos].number
    }

    /// The id of the window shown at index `number`.
    pub fn window_id_at(&self, number: usize) -> Option<usize> {
        self.position_of(number).map(|pos| self.links[pos].window)
    }

    /// Shows the window `window` (an id from `window_id_at`) at index `number`, numbered as
    /// `install_window` numbers windows. Returns the index it took.
    pub fn link_restored(&mut self, window: usize, number: usize) -> usize {
        let number = self.number_for(number);
        let pos = self.insert_link(window, number);
        self.links[pos].number
    }

    /// Makes the link at position `pos` the active one. Leaving a window unzooms it; the alerts of
    /// the index are cleared.
    fn set_active(&mut self, pos: usize) {
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
            let prev = if self.active_idx == 0 {
                self.links.len() - 1
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

    /// Every index with the window it shows, in number order.
    pub fn linked(&self) -> impl Iterator<Item = (&Link, &Window)> + '_ {
        self.links
            .iter()
            .filter_map(|link| Some((link, self.window_by_id(link.window)?)))
    }

    pub fn window_tabs(&self) -> Vec<WindowTab> {
        self.links
            .iter()
            .enumerate()
            .filter_map(|(i, link)| {
                let w = self.window_by_id(link.window)?;
                let marked_pane = self.marked_pane.as_ref().filter(|m| w.contains(m)).cloned();
                Some(WindowTab {
                    index: link.number,
                    name: w.name.clone(),
                    is_active: i == self.active_idx,
                    is_last: Some(i) == self.last_window,
                    has_activity: link.has_activity,
                    has_bell: link.has_bell,
                    has_silence: link.has_silence,
                    synchronized: w.synchronized,
                    zoomed: w.zoomed,
                    focused: w.focused.clone(),
                    has_marked: marked_pane.is_some(),
                    marked_pane,
                    agent_badge: None,
                    linked: self.numbers_of(link.window),
                })
            })
            .collect()
    }

    /// Sets an alert flag on every index showing the window of `pane` but the active one. True when
    /// a flag was newly set.
    fn mark_alert(&mut self, pane: &str, flag: impl Fn(&mut Link) -> &mut bool) -> bool {
        let active = self.active_idx;
        let mut marked = false;
        for pos in self.links_showing(pane) {
            if pos == active {
                continue;
            }
            let target = flag(&mut self.links[pos]);
            if !*target {
                *target = true;
                marked = true;
            }
        }
        marked
    }

    pub fn mark_activity(&mut self, pane_name: &str) -> bool {
        self.mark_alert(pane_name, |l| &mut l.has_activity)
    }

    pub fn mark_bell(&mut self, pane_name: &str) -> bool {
        self.mark_alert(pane_name, |l| &mut l.has_bell)
    }

    /// Records output from a pane: the silence timer of every index showing its window restarts and
    /// their silence flags clear.
    pub fn note_output(&mut self, pane_name: &str, now: Instant) {
        for pos in self.links_showing(pane_name) {
            let link = &mut self.links[pos];
            link.last_output = Some(now);
            link.has_silence = false;
        }
    }

    /// Flags background indices whose windows have been quiet for `secs` seconds
    /// (`monitor-silence`). Returns the numbers of the indices flagged now.
    pub fn expire_silence(&mut self, secs: u64, now: Instant) -> Vec<usize> {
        let active = self.active_idx;
        let quiet = Duration::from_secs(secs);
        let mut flagged = Vec::new();
        for (pos, link) in self.links.iter_mut().enumerate() {
            if pos != active
                && !link.has_silence
                && link
                    .last_output
                    .is_some_and(|t| now.duration_since(t) >= quiet)
            {
                link.has_silence = true;
                flagged.push(link.number);
            }
        }
        flagged
    }

    /// When the next background index goes quiet for `secs` seconds, if any is still counting.
    pub fn next_silence(&self, secs: u64) -> Option<Instant> {
        let quiet = Duration::from_secs(secs);
        self.links
            .iter()
            .enumerate()
            .filter(|(i, l)| *i != self.active_idx && !l.has_silence)
            .filter_map(|(_, l)| l.last_output)
            .map(|t| t + quiet)
            .min()
    }

    /// Turns `synchronize-panes` on or off for the window at index `number` (the active window when
    /// `None`); `on` None toggles it.
    pub fn synchronize(&mut self, number: Option<usize>, on: Option<bool>) -> Result<(), String> {
        let win = self.window_mut_at(number)?;
        win.synchronized = on.unwrap_or(!win.synchronized);
        Ok(())
    }

    pub fn active_window(&self) -> Option<&Window> {
        self.window_at(self.active_idx)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        let pos = self.active_idx;
        self.window_at_mut(pos)
    }

    /// The position in `links` of the index showing pane `name`: the active index when it shows it,
    /// else the first index that does.
    pub fn window_of(&self, name: &str) -> Option<usize> {
        let shows = |pos: usize| self.window_at(pos).is_some_and(|w| w.contains(name));
        if shows(self.active_idx) {
            return Some(self.active_idx);
        }
        (0..self.links.len()).find(|&pos| shows(pos))
    }

    /// The number of the index showing pane `name`.
    pub fn number_of(&self, name: &str) -> Option<usize> {
        self.window_of(name).map(|pos| self.links[pos].number)
    }

    /// The number of the active index, if there is one.
    pub fn active_number(&self) -> Option<usize> {
        self.links.get(self.active_idx).map(|l| l.number)
    }

    /// The panes of the window at index `number`, empty when there is no such index.
    pub fn leaves_at(&self, number: usize) -> Vec<String> {
        self.position_of(number)
            .and_then(|pos| self.window_at(pos))
            .map(Window::leaves)
            .unwrap_or_default()
    }

    /// The focused pane of the window at index `number`.
    pub fn focused_at(&self, number: usize) -> Option<String> {
        self.position_of(number)
            .and_then(|pos| self.window_at(pos))
            .and_then(|w| w.focused.clone())
    }

    /// Number of panes across all windows.
    pub fn pane_count(&self) -> usize {
        self.windows.iter().map(|w| w.leaves().len()).sum()
    }

    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        self.add_or_split_with_focus(node_name, dir, true);
    }

    pub fn add_or_split_with_focus(&mut self, node_name: String, dir: Direction, focus: bool) {
        if self.links.is_empty() {
            let win_name = node_name.clone();
            self.new_window_with_focus(win_name, node_name, focus);
            return;
        }
        if let Some(win) = self.active_window_mut() {
            win.split_at(None, node_name, dir, None, focus);
        }
    }

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

    pub fn remove(&mut self, node_name: &str) {
        if self.marked_pane.as_deref() == Some(node_name) {
            self.marked_pane = None;
        }
        for win in &mut self.windows {
            win.remove(node_name);
        }
        self.drop_empty_windows();
    }

    /// Drops the windows left without panes, with every index that showed them.
    fn drop_empty_windows(&mut self) {
        let active = self.links.get(self.active_idx).map(|l| l.id);
        let last = self
            .last_window
            .and_then(|i| self.links.get(i))
            .map(|l| l.id);
        let emptied: Vec<usize> = self
            .windows
            .iter()
            .filter(|w| w.root.is_none())
            .map(|w| w.id)
            .collect();
        self.windows.retain(|w| w.root.is_some());
        self.links.retain(|l| !emptied.contains(&l.window));
        self.restore_positions(active, last);
        if !emptied.is_empty() {
            self.renumber_if_enabled();
        }
    }

    pub fn focus_next(&mut self) {
        if let Some(win) = self.active_window_mut() {
            win.focus_next();
            win.zoomed = false;
        }
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

    pub fn rename_window(&mut self, number: usize, name: String) -> bool {
        let Some(pos) = self.position_of(number) else {
            return false;
        };
        match self.window_at_mut(pos) {
            Some(win) => {
                win.name = name;
                win.renamed = true;
                true
            }
            None => false,
        }
    }

    /// Sets the automatic name of the window at index `number` unless it was renamed explicitly.
    pub fn set_auto_name(&mut self, number: usize, name: Option<String>) {
        if let Some(pos) = self.position_of(number)
            && let Some(win) = self.window_at_mut(pos)
            && !win.renamed
        {
            win.name = name.unwrap_or_else(|| win.label.clone());
        }
    }

    /// Kills the window at index `number` with every index it is linked at, and returns its panes.
    /// With renumber-windows on, the indices after it are renumbered.
    pub fn kill_window(&mut self, number: usize) -> Option<Vec<String>> {
        let pos = self.position_of(number)?;
        let id = self.links[pos].window;
        let panes = self.kill_body(id);
        self.renumber_if_enabled();
        Some(panes)
    }

    /// Removes the window `id` and every link to it. Returns its panes.
    fn kill_body(&mut self, id: usize) -> Vec<String> {
        let panes = self
            .window_by_id(id)
            .map(Window::leaves)
            .unwrap_or_default();
        if self.marked_pane.as_ref().is_some_and(|m| panes.contains(m)) {
            self.marked_pane = None;
        }
        let gone: Vec<usize> = self
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| l.window == id)
            .map(|(pos, _)| pos)
            .collect();
        self.windows.retain(|w| w.id != id);
        self.unlink_positions(&gone);
        panes
    }

    /// Removes the links at the positions `gone` (ascending). The active and last indices move with
    /// their links; when the active one is removed, the index that takes its place becomes active.
    fn unlink_positions(&mut self, gone: &[usize]) {
        let shift = |p: usize| p - gone.iter().filter(|&&g| g < p).count();
        let active = shift(self.active_idx);
        let last = self.last_window.filter(|l| !gone.contains(l)).map(shift);
        let mut pos = 0;
        self.links.retain(|_| {
            let keep = !gone.contains(&pos);
            pos += 1;
            keep
        });
        self.active_idx = active.min(self.links.len().saturating_sub(1));
        self.last_window = last;
    }

    /// Removes one index (`pos` in `links`). The window it showed is killed when that was its only
    /// index, which needs `kill`; returns the panes of a killed window.
    fn unlink_at(&mut self, pos: usize, kill: bool) -> Result<Vec<String>, String> {
        let (id, number) = (self.links[pos].window, self.links[pos].number);
        if self.links.iter().filter(|l| l.window == id).count() > 1 {
            self.unlink_positions(&[pos]);
            return Ok(Vec::new());
        }
        if !kill {
            return Err(format!(
                "window {number} is linked to no other index: use -k to kill it"
            ));
        }
        Ok(self.kill_body(id))
    }

    /// Removes the index `number` (tmux `unlink-window`). Its window stays at its other indices;
    /// a window with no other index is killed only with `kill`. Returns the panes of a killed
    /// window.
    pub fn unlink_window(&mut self, number: usize, kill: bool) -> Result<Vec<String>, String> {
        let pos = self
            .position_of(number)
            .ok_or_else(|| window_not_found(number))?;
        let panes = self.unlink_at(pos, kill)?;
        self.renumber_if_enabled();
        Ok(panes)
    }

    /// Shows the window at index `src` at index `dst` as well (tmux `link-window`). Without `dst` the
    /// lowest free index is taken. With `after` or `before`, the window goes next to `dst` and the
    /// indices in the way move up. An occupied index is refused unless `kill`, which unlinks what is
    /// there first. Returns the new index and the panes of a window that `kill` destroyed.
    pub fn link_window(
        &mut self,
        src: usize,
        dst: Option<usize>,
        place: LinkPlace,
    ) -> Result<(usize, Vec<String>), String> {
        let src_pos = self.position_of(src).ok_or_else(|| window_not_found(src))?;
        let window = self.links[src_pos].window;
        let shuffles = place.after || place.before;
        if !shuffles && dst == Some(src) {
            return Err(format!("same index: {src}"));
        }
        let number = if shuffles {
            let base = dst.or_else(|| self.active_number()).unwrap_or(0);
            self.open_index(base + usize::from(place.after))
        } else {
            dst.unwrap_or_else(|| self.lowest_free_number())
        };
        let mut destroyed = Vec::new();
        if let Some(pos) = self.position_of(number) {
            if !place.kill {
                return Err(format!("index in use: {number}"));
            }
            destroyed = self.unlink_at(pos, true)?;
        }
        let pos = self.insert_link(window, number);
        if !place.detached {
            self.set_active(pos);
        }
        Ok((number, destroyed))
    }

    /// Frees index `start` for a new link: the run of occupied indices from `start` moves up one
    /// (tmux `winlink_shuffle_up`). Returns `start`.
    fn open_index(&mut self, start: usize) -> usize {
        let mut end = start;
        while self.position_of(end).is_some() {
            end += 1;
        }
        for link in self.links.iter_mut() {
            if (start..end).contains(&link.number) {
                link.number += 1;
            }
        }
        start
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
        let Some(pos) = self.window_of(name) else {
            return false;
        };
        let id = self.links[pos].window;
        let Some(win) = self.window_by_id_mut(id) else {
            return false;
        };
        if win.leaves().len() <= 1 {
            return false;
        }
        win.remove(name);
        let new_id = self.push_window(|id| Window::new(id, name.to_string(), name.to_string()));
        let number = self.lowest_free_number();
        let pos = self.insert_link(new_id, number);
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
        let from_id = self.links[from].window;
        if let Some(win) = self.window_by_id_mut(from_id) {
            win.remove(src);
        }
        let to = self.window_of(target).unwrap_or(from);
        let to_id = self.links[to].window;
        let joined = self
            .window_by_id_mut(to_id)
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

    /// Exchanges the numbers of two windows (tmux `swap-window`); the active and last index stay on
    /// the same windows.
    pub fn swap_windows(&mut self, a: usize, b: usize) -> Result<(), String> {
        let pa = self.position_of(a).ok_or_else(|| window_not_found(a))?;
        let pb = self.position_of(b).ok_or_else(|| window_not_found(b))?;
        if pa != pb {
            self.reorder(|links| {
                let number = links[pa].number;
                links[pa].number = links[pb].number;
                links[pb].number = number;
                links.sort_by_key(|l| l.number);
            });
        }
        Ok(())
    }

    /// Moves index `from` to number `to` (tmux `move-window`). With renumber-windows on, `to` is a
    /// position and the indices are renumbered afterwards; otherwise `to` must be free.
    pub fn move_window(&mut self, from: usize, to: usize) -> Result<(), String> {
        let from_pos = self
            .position_of(from)
            .ok_or_else(|| window_not_found(from))?;
        if self.renumber_windows {
            if to >= self.links.len() {
                return Err(window_not_found(to));
            }
            self.reorder(|links| {
                let link = links.remove(from_pos);
                links.insert(to, link);
            });
            self.compact_numbers();
        } else if from != to {
            if self.position_of(to).is_some() {
                return Err(format!("Window {to} already exists"));
            }
            self.reorder(|links| {
                links[from_pos].number = to;
                links.sort_by_key(|l| l.number);
            });
        }
        Ok(())
    }

    /// Changes the order of the links, keeping the active and last index on their links.
    fn reorder(&mut self, change: impl FnOnce(&mut Vec<Link>)) {
        let active = self.links.get(self.active_idx).map(|l| l.id);
        let last = self
            .last_window
            .and_then(|i| self.links.get(i))
            .map(|l| l.id);
        change(&mut self.links);
        self.restore_positions(active, last);
    }

    /// Points the active and last indices at the links with ids `active` and `last`. When the active
    /// link is gone, the last link becomes active.
    fn restore_positions(&mut self, active: Option<usize>, last: Option<usize>) {
        let position = |id: Option<usize>, links: &[Link]| {
            id.and_then(|id| links.iter().position(|l| l.id == id))
        };
        let fallback = self.links.len().saturating_sub(1);
        self.active_idx = position(active, &self.links).unwrap_or(fallback);
        self.last_window = position(last, &self.links);
    }

    /// The position in `links` of the index `number`.
    fn position_of(&self, number: usize) -> Option<usize> {
        self.links.binary_search_by_key(&number, |l| l.number).ok()
    }

    /// The lowest number no index has: the first gap in the ascending numbers.
    fn lowest_free_number(&self) -> usize {
        self.links
            .iter()
            .enumerate()
            .find(|(i, l)| l.number != *i)
            .map_or(self.links.len(), |(i, _)| i)
    }

    /// The number a window restored or added at `wanted` takes: `wanted` when it is free and
    /// renumber-windows is off, else the lowest free one.
    fn number_for(&self, wanted: usize) -> usize {
        if self.renumber_windows || self.position_of(wanted).is_some() {
            self.lowest_free_number()
        } else {
            wanted
        }
    }

    /// Adds a window, built from its id, and returns the id.
    fn push_window(&mut self, make: impl FnOnce(usize) -> Window) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        let mut window = make(id);
        window.synchronized = self.synchronize_new;
        self.windows.push(window);
        id
    }

    /// Adds an index showing window `window` at number `number`, in number order. Returns its
    /// position.
    fn insert_link(&mut self, window: usize, number: usize) -> usize {
        let id = self.next_link_id;
        self.next_link_id += 1;
        let pos = self.links.partition_point(|l| l.number < number);
        let link = Link::new(id, number, window);
        self.reorder(|links| links.insert(pos, link));
        pos
    }

    fn renumber_if_enabled(&mut self) {
        if self.renumber_windows {
            self.compact_numbers();
        }
    }

    /// Numbers the indices 0, 1, ... in order (tmux `move-window -r`).
    pub fn compact_numbers(&mut self) {
        for (i, link) in self.links.iter_mut().enumerate() {
            link.number = i;
        }
    }

    /// The numbers of every index showing window `id`, in number order.
    fn numbers_of(&self, id: usize) -> Vec<usize> {
        self.links
            .iter()
            .filter(|l| l.window == id)
            .map(|l| l.number)
            .collect()
    }

    /// The positions in `links` of the indices showing the window that holds pane `name`.
    fn links_showing(&self, name: &str) -> Vec<usize> {
        self.links
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                self.window_by_id(l.window)
                    .is_some_and(|w| w.contains(name))
            })
            .map(|(pos, _)| pos)
            .collect()
    }

    fn window_by_id(&self, id: usize) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn window_by_id_mut(&mut self, id: usize) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The window shown at position `pos` of `links`.
    fn window_at(&self, pos: usize) -> Option<&Window> {
        self.links
            .get(pos)
            .and_then(|link| self.window_by_id(link.window))
    }

    fn window_at_mut(&mut self, pos: usize) -> Option<&mut Window> {
        let id = self.links.get(pos)?.window;
        self.window_by_id_mut(id)
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
        let pos = target
            .and_then(|t| self.window_of(t))
            .unwrap_or(self.active_idx);
        self.window_at(pos).is_some_and(|w| w.zoomed)
    }

    /// Zooms the window holding pane `pane`.
    pub fn zoom_window_of(&mut self, pane: &str) {
        if let Some(pos) = self.window_of(pane)
            && let Some(win) = self.window_at_mut(pos)
        {
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

    /// The size `resize-window` pinned the window at index `number` to (the active window when
    /// `None`), if it has one.
    pub fn manual_size_at(&self, number: Option<usize>) -> Option<(u16, u16)> {
        let pos = match number {
            Some(n) => self.position_of(n)?,
            None => self.active_idx,
        };
        self.window_at(pos)?.manual_size
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

    /// The window at index `number` (the active one when `None`), for a change to it.
    fn window_mut_at(&mut self, number: Option<usize>) -> Result<&mut Window, String> {
        let pos = match number {
            Some(n) => self.position_of(n).ok_or_else(|| window_not_found(n))?,
            None => self.active_idx,
        };
        self.window_at_mut(pos)
            .ok_or_else(|| "No window".to_string())
    }

    pub fn resize_split(
        &mut self,
        name: &str,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    ) -> bool {
        let Some(pos) = self.window_of(name) else {
            return false;
        };
        match self.window_at_mut(pos) {
            Some(win) => win.resize_split(name, side, cells, Rect::new(0, 0, width, height)),
            None => false,
        }
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
