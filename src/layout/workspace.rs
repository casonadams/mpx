use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use super::tree::{Direction, LayoutNode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub root: Option<LayoutNode>,
    pub focused: Option<String>,
}

impl Workspace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            root: None,
            focused: None,
        }
    }

    pub fn add_or_split(&mut self, node_name: String, dir: Direction) {
        if self.root.is_none() {
            self.root = Some(LayoutNode::leaf(node_name.clone()));
            self.focused = Some(node_name);
            return;
        }

        let target = self.focused.clone();
        if let (Some(root), Some(focus)) = (&mut self.root, target) {
            if root.split_leaf(&focus, node_name.clone(), dir) {
                self.focused = Some(node_name);
                return;
            }
        }

        // Fallback: if focused not found, split first available leaf
        if let Some(root) = &mut self.root {
            let leaves = root.all_leaves();
            if let Some(first) = leaves.first() {
                root.split_leaf(first, node_name.clone(), dir);
                self.focused = Some(node_name);
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

    pub fn compute_layout(&self, width: u16, height: u16) -> Vec<(String, Rect, bool)> {
        let bounds = Rect::new(0, 0, width, height);
        let rects = match &self.root {
            Some(r) => r.compute_rects(bounds),
            None => return Vec::new(),
        };

        rects
            .into_iter()
            .map(|(name, rect)| {
                let is_focused = self.focused.as_deref() == Some(&name);
                (name, rect, is_focused)
            })
            .collect()
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
        assert_eq!(p2.1, Rect::new(50, 0, 50, 50));
        assert!(p2.2, "pane2 should have focus after split");
    }

    #[test]
    fn test_workspace_focus_cycle() {
        let mut ws = Workspace::new("main".to_string());
        ws.add_or_split("pane1".to_string(), Direction::Horizontal);
        ws.add_or_split("pane2".to_string(), Direction::Horizontal);

        assert_eq!(ws.focused.as_deref(), Some("pane2"));
        ws.focus_next();
        assert_eq!(ws.focused.as_deref(), Some("pane1"));
        ws.focus_next();
        assert_eq!(ws.focused.as_deref(), Some("pane2"));
        ws.focus_prev();
        assert_eq!(ws.focused.as_deref(), Some("pane1"));
    }

    #[test]
    fn test_workspace_multiple_consecutive_splits() {
        let mut ws = Workspace::new("main".to_string());
        ws.add_or_split("pane1".to_string(), Direction::Horizontal);
        ws.add_or_split("pane2".to_string(), Direction::Vertical);
        ws.add_or_split("pane3".to_string(), Direction::Horizontal);
        ws.add_or_split("pane4".to_string(), Direction::Vertical);
        ws.add_or_split("pane5".to_string(), Direction::Horizontal);

        let layout = ws.compute_layout(100, 50);
        assert_eq!(layout.len(), 5);
        let names: Vec<_> = layout.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(names, vec!["pane1", "pane2", "pane3", "pane4", "pane5"]);
    }
}
