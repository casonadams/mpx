use super::Window;
use crate::layout::geometry::Rect;
use crate::layout::tree::{Direction, Divider, LayoutNode, preset_tree};
use crate::protocol::{LayoutPreset, Side};

impl Window {
    pub fn new(id: usize, name: String, root_node: String) -> Self {
        let focused = Some(root_node.clone());
        Self::with_layout(id, name, LayoutNode::leaf(root_node), focused)
    }

    /// A window named `label` showing `root`, with `focused` as its active pane.
    pub(super) fn with_layout(
        id: usize,
        label: String,
        root: LayoutNode,
        focused: Option<String>,
    ) -> Self {
        Self {
            id,
            name: label.clone(),
            label,
            renamed: false,
            root: Some(root),
            focused,
            focus_history: Vec::new(),
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

    /// Removes a pane. When it was focused, focus goes to the most recently focused pane left,
    /// else to the pane before it in layout order, else the one after (as tmux does).
    pub fn remove(&mut self, node_name: &str) {
        if self.contains(node_name) {
            self.zoomed = false;
        }
        let position = self.leaves().iter().position(|n| n == node_name);
        if let Some(root) = self.root.take() {
            self.root = root.remove_leaf(node_name);
        }
        self.focus_history.retain(|n| n != node_name);
        if self.focused.as_deref() == Some(node_name) {
            let leaves = self.leaves();
            self.focused = self
                .recent_panes()
                .into_iter()
                .next()
                .or_else(|| leaves.get(position.unwrap_or(0).saturating_sub(1)).cloned());
            self.focus_history
                .retain(|n| Some(n) != self.focused.as_ref());
        }
    }

    /// The panes focused before the current one, most recent first.
    pub fn recent_panes(&self) -> Vec<String> {
        self.focus_history
            .iter()
            .filter(|n| self.contains(n))
            .cloned()
            .collect()
    }

    /// Focuses a leaf; the previously focused pane becomes the most recent in the focus history.
    pub fn set_focus(&mut self, name: &str) -> bool {
        if !self.contains(name) {
            return false;
        }
        if self.focused.as_deref() != Some(name) {
            self.focus_history.retain(|n| n != name);
            if let Some(previous) = self.focused.replace(name.to_string()) {
                self.focus_history.insert(0, previous);
            }
        }
        true
    }

    pub fn focus_last(&mut self) -> bool {
        match self.recent_panes().into_iter().next() {
            Some(last) => self.set_focus(&last),
            None => false,
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
        self.root
            .as_mut()
            .is_some_and(|root| root.resize_border(name, side, cells, bounds))
    }

    pub fn compute_layout(&self, bounds: Rect) -> (Vec<(String, Rect, bool)>, Vec<Divider>) {
        let Some(root) = &self.root else {
            return (Vec::new(), Vec::new());
        };
        let (rects, dividers) = root.compute_layout(bounds);
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
