use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use super::tree::{Direction, Divider, LayoutNode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTab {
    pub index: usize,
    pub name: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub id: usize,
    pub name: String,
    pub root: Option<LayoutNode>,
    pub focused: Option<String>,
}

impl Window {
    pub fn new(id: usize, name: String, root_node: String) -> Self {
        Self {
            id,
            name,
            root: Some(LayoutNode::leaf(root_node.clone())),
            focused: Some(root_node),
        }
    }

    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        self.add_or_split_with_focus(node_name, dir, true);
    }

    pub fn add_or_split_with_focus(&mut self, node_name: String, dir: Direction, focus: bool) {
        if self.root.is_none() {
            self.root = Some(LayoutNode::leaf(node_name.clone()));
            self.focused = Some(node_name);
            return;
        }

        let target = self.focused.clone();
        if let (Some(root), Some(focus_target)) = (&mut self.root, target) {
            if root.split_leaf(&focus_target, node_name.clone(), dir) {
                if focus {
                    self.focused = Some(node_name);
                }
                return;
            }
        }

        if let Some(root) = &mut self.root {
            let leaves = root.all_leaves();
            if let Some(first) = leaves.first() {
                root.split_leaf(first, node_name.clone(), dir);
                if focus {
                    self.focused = Some(node_name);
                }
            }
        }
    }

    pub fn remove(&mut self, node_name: &str) {
        if let Some(root) = self.root.take() {
            self.root = root.remove_leaf(node_name);
        }

        if self.focused.as_deref() == Some(node_name) {
            self.focused = self.root.as_ref().and_then(|r| r.all_leaves().into_iter().next());
        }
    }

    pub fn focus_next(&mut self) {
        let leaves = match &self.root {
            Some(r) => r.all_leaves(),
            None => return,
        };
        if leaves.is_empty() {
            self.focused = None;
            return;
        }

        let current_idx = self
            .focused
            .as_ref()
            .and_then(|f| leaves.iter().position(|l| l == f))
            .unwrap_or(0);

        let next_idx = (current_idx + 1) % leaves.len();
        self.focused = Some(leaves[next_idx].clone());
    }

    pub fn focus_prev(&mut self) {
        let leaves = match &self.root {
            Some(r) => r.all_leaves(),
            None => return,
        };
        if leaves.is_empty() {
            self.focused = None;
            return;
        }

        let current_idx = self
            .focused
            .as_ref()
            .and_then(|f| leaves.iter().position(|l| l == f))
            .unwrap_or(0);

        let prev_idx = if current_idx == 0 {
            leaves.len() - 1
        } else {
            current_idx - 1
        };
        self.focused = Some(leaves[prev_idx].clone());
    }

    pub fn focus(&mut self, name: &str) {
        if let Some(root) = &self.root {
            if root.all_leaves().iter().any(|l| l == name) {
                self.focused = Some(name.to_string());
            }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub windows: Vec<Window>,
    pub active_idx: usize,
    pub next_window_id: usize,
}

impl Workspace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            windows: Vec::new(),
            active_idx: 0,
            next_window_id: 0,
        }
    }

    pub fn new_window(&mut self, window_name: String, root_node: String) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        let window = Window::new(id, window_name, root_node);
        self.windows.push(window);
        self.active_idx = self.windows.len().saturating_sub(1);
        id
    }

    pub fn next_window(&mut self) {
        if !self.windows.is_empty() {
            self.active_idx = (self.active_idx + 1) % self.windows.len();
        }
    }

    pub fn prev_window(&mut self) {
        if !self.windows.is_empty() {
            self.active_idx = if self.active_idx == 0 {
                self.windows.len().saturating_sub(1)
            } else {
                self.active_idx - 1
            };
        }
    }

    pub fn select_window(&mut self, idx: usize) -> bool {
        if idx < self.windows.len() {
            self.active_idx = idx;
            true
        } else {
            false
        }
    }

    pub fn window_tabs(&self) -> Vec<WindowTab> {
        self.windows
            .iter()
            .enumerate()
            .map(|(i, w)| WindowTab {
                index: i,
                name: w.name.clone(),
                is_active: i == self.active_idx,
            })
            .collect()
    }

    pub fn active_window(&self) -> Option<&Window> {
        self.windows.get(self.active_idx)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(self.active_idx)
    }

    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        self.add_or_split_with_focus(node_name, dir, true);
    }

    pub fn add_or_split_with_focus(&mut self, node_name: String, dir: Direction, focus: bool) {
        if self.windows.is_empty() {
            let win_name = node_name.clone();
            self.new_window(win_name, node_name);
            return;
        }

        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.add_or_split_with_focus(node_name, dir, focus);
        }
    }

    pub fn remove(&mut self, node_name: &str) {
        for win in &mut self.windows {
            win.remove(node_name);
        }

        // Close windows that have no remaining panes
        let mut i = 0;
        while i < self.windows.len() {
            if self.windows[i].root.is_none() {
                self.windows.remove(i);
                if self.active_idx >= self.windows.len() && !self.windows.is_empty() {
                    self.active_idx = self.windows.len() - 1;
                }
            } else {
                i += 1;
            }
        }
    }

    pub fn focus_next(&mut self) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.focus_next();
        }
    }

    pub fn focus_prev(&mut self) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.focus_prev();
        }
    }

    pub fn focus(&mut self, name: &str) {
        for (i, win) in self.windows.iter_mut().enumerate() {
            if let Some(root) = &win.root {
                if root.all_leaves().iter().any(|l| l == name) {
                    win.focus(name);
                    self.active_idx = i;
                    break;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_single_node() {
        let mut ws = Workspace::new("main".to_string());
        ws.add_or_split("pane1".to_string(), Direction::Horizontal);

        let layout = ws.compute_layout(100, 50);
        assert_eq!(layout.len(), 1);
        let (name, rect, focused) = &layout[0];
        assert_eq!(name, "pane1");
        assert_eq!(*rect, Rect::new(0, 0, 100, 50));
        assert!(focused);
    }

    #[test]
    fn test_workspace_split_horizontal() {
        let mut ws = Workspace::new("main".to_string());
        ws.add_or_split("pane1".to_string(), Direction::Horizontal);
        ws.add_or_split("pane2".to_string(), Direction::Horizontal);

        let layout = ws.compute_layout(100, 50);
        assert_eq!(layout.len(), 2);

        let p1 = layout.iter().find(|(n, ..)| n == "pane1").unwrap();
        let p2 = layout.iter().find(|(n, ..)| n == "pane2").unwrap();

        assert_eq!(p1.1, Rect::new(0, 0, 50, 50));
        assert_eq!(p2.1, Rect::new(51, 0, 49, 50));
        assert!(p2.2, "pane2 should have focus after split");
    }

    #[test]
    fn test_workspace_multi_windows() {
        let mut ws = Workspace::new("main".to_string());
        ws.new_window("win0".into(), "shell0".into());
        ws.new_window("win1".into(), "shell1".into());

        assert_eq!(ws.windows.len(), 2);
        assert_eq!(ws.active_idx, 1);

        // Previous window (p)
        ws.prev_window();
        assert_eq!(ws.active_idx, 0);

        // Next window (n)
        ws.next_window();
        assert_eq!(ws.active_idx, 1);

        // Window tabs
        let tabs = ws.window_tabs();
        assert_eq!(tabs.len(), 2);
        assert!(!tabs[0].is_active);
        assert!(tabs[1].is_active);
    }
}
