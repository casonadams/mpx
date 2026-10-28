use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use crate::protocol::{LayoutPreset, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Horizontal, // Side-by-side (left / right)
    Vertical,   // Stacked (top / bottom)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divider {
    pub is_vertical: bool, // true for │, false for ─
    pub x: u16,
    pub y: u16,
    pub length: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutNode {
    Leaf {
        name: String,
    },
    Split {
        direction: Direction,
        ratio: f32,
        left: Box<LayoutNode>,
        right: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(name: String) -> Self {
        Self::Leaf { name }
    }

    pub fn compute_rects(&self, bounds: Rect) -> Vec<(String, Rect)> {
        self.compute_layout(bounds).0
    }

    pub fn compute_layout(&self, bounds: Rect) -> (Vec<(String, Rect)>, Vec<Divider>) {
        let mut panes = Vec::new();
        let mut dividers = Vec::new();
        self.collect_layout(bounds, &mut panes, &mut dividers);
        (panes, dividers)
    }

    fn collect_layout(
        &self,
        bounds: Rect,
        panes: &mut Vec<(String, Rect)>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Self::Leaf { name } => {
                panes.push((name.clone(), bounds));
            }
            Self::Split {
                direction,
                ratio,
                left,
                right,
            } => {
                let (r1, r2) = child_bounds(*direction, *ratio, bounds);
                match direction {
                    Direction::Horizontal => dividers.push(Divider {
                        is_vertical: true,
                        x: r1.x + r1.width,
                        y: bounds.y,
                        length: bounds.height,
                    }),
                    Direction::Vertical => dividers.push(Divider {
                        is_vertical: false,
                        x: bounds.x,
                        y: r1.y + r1.height,
                        length: bounds.width,
                    }),
                }
                left.collect_layout(r1, panes, dividers);
                right.collect_layout(r2, panes, dividers);
            }
        }
    }

    pub fn split_leaf(&mut self, target: &str, new_name: String, dir: Direction) -> bool {
        self.split_leaf_with(target, new_name, dir, 0.5)
    }

    /// Replaces the leaf `target` with a split; `ratio` is the share kept by the existing pane.
    pub fn split_leaf_with(
        &mut self,
        target: &str,
        new_name: String,
        dir: Direction,
        ratio: f32,
    ) -> bool {
        match self {
            Self::Leaf { name } if name == target => {
                let old_leaf = LayoutNode::Leaf {
                    name: target.to_string(),
                };
                let new_leaf = LayoutNode::Leaf { name: new_name };
                *self = LayoutNode::Split {
                    direction: dir,
                    ratio,
                    left: Box::new(old_leaf),
                    right: Box::new(new_leaf),
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { left, right, .. } => {
                left.split_leaf_with(target, new_name.clone(), dir, ratio)
                    || right.split_leaf_with(target, new_name, dir, ratio)
            }
        }
    }

    pub fn all_leaves(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, acc: &mut Vec<String>) {
        match self {
            Self::Leaf { name } => acc.push(name.clone()),
            Self::Split { left, right, .. } => {
                left.collect_leaves(acc);
                right.collect_leaves(acc);
            }
        }
    }

    pub fn has_leaf(&self, target: &str) -> bool {
        match self {
            Self::Leaf { name } => name == target,
            Self::Split { left, right, .. } => left.has_leaf(target) || right.has_leaf(target),
        }
    }

    /// Applies `f` to every leaf name.
    pub fn map_leaves(&mut self, f: &mut impl FnMut(&mut String)) {
        match self {
            Self::Leaf { name } => f(name),
            Self::Split { left, right, .. } => {
                left.map_leaves(f);
                right.map_leaves(f);
            }
        }
    }

    pub fn remove_leaf(self, target: &str) -> Option<LayoutNode> {
        match self {
            Self::Leaf { name } => {
                if name == target {
                    None
                } else {
                    Some(Self::Leaf { name })
                }
            }
            Self::Split {
                direction,
                ratio,
                left,
                right,
            } => {
                let left_res = left.remove_leaf(target);
                let right_res = right.remove_leaf(target);

                match (left_res, right_res) {
                    (Some(l), Some(r)) => Some(Self::Split {
                        direction,
                        ratio,
                        left: Box::new(l),
                        right: Box::new(r),
                    }),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                }
            }
        }
    }

    /// Moves the nearest split border that separates `target` from the `side` by `cells`
    /// (tmux `resize-pane -L/-R/-U/-D`: the target grows towards `side`). Returns false when no
    /// such border exists.
    pub fn resize_border(&mut self, target: &str, side: Side, cells: u16, bounds: Rect) -> bool {
        let (dir, toward_first, sign) = match side {
            Side::Right => (Direction::Horizontal, true, 1),
            Side::Left => (Direction::Horizontal, false, -1),
            Side::Down => (Direction::Vertical, true, 1),
            Side::Up => (Direction::Vertical, false, -1),
        };
        let delta = sign * i32::from(cells);
        self.resize_nearest(target, dir, toward_first, delta, bounds)
    }

    fn resize_nearest(
        &mut self,
        target: &str,
        dir: Direction,
        toward_first: bool,
        delta: i32,
        bounds: Rect,
    ) -> bool {
        let Self::Split {
            direction,
            ratio,
            left,
            right,
        } = self
        else {
            return false;
        };
        let (lb, rb) = child_bounds(*direction, *ratio, bounds);
        let (in_left, in_right) = (left.has_leaf(target), right.has_leaf(target));
        if in_left && left.resize_nearest(target, dir, toward_first, delta, lb) {
            return true;
        }
        if in_right && right.resize_nearest(target, dir, toward_first, delta, rb) {
            return true;
        }
        let separates = if toward_first { in_left } else { in_right };
        if *direction != dir || !separates {
            return false;
        }
        let (total, first_size) = match dir {
            Direction::Horizontal => (bounds.width, lb.width),
            Direction::Vertical => (bounds.height, lb.height),
        };
        let avail = i32::from(total.saturating_sub(1));
        if avail == 0 {
            return false;
        }
        let new_first = (i32::from(first_size) + delta).clamp(0, avail);
        *ratio = (new_first as f32 / avail as f32).clamp(0.1, 0.9);
        true
    }
}

fn child_bounds(direction: Direction, ratio: f32, bounds: Rect) -> (Rect, Rect) {
    match direction {
        Direction::Horizontal => bounds.split_horizontal(ratio),
        Direction::Vertical => bounds.split_vertical(ratio),
    }
}

/// Rebuilds a window tree from its leaves in order.
pub fn preset_tree(preset: LayoutPreset, leaves: &[String]) -> Option<LayoutNode> {
    let (first, rest) = leaves.split_first()?;
    let leaf = |name: &String| LayoutNode::leaf(name.clone());
    match preset {
        LayoutPreset::EvenHorizontal => {
            stack(leaves.iter().map(leaf).collect(), Direction::Horizontal)
        }
        LayoutPreset::EvenVertical => stack(leaves.iter().map(leaf).collect(), Direction::Vertical),
        LayoutPreset::MainVertical => {
            main_split(first, rest, Direction::Horizontal, Direction::Vertical)
        }
        LayoutPreset::MainHorizontal => {
            main_split(first, rest, Direction::Vertical, Direction::Horizontal)
        }
        LayoutPreset::Tiled => {
            let cols = (leaves.len() as f64).sqrt().ceil() as usize;
            let rows = leaves
                .chunks(cols.max(1))
                .map(|row| stack(row.iter().map(leaf).collect(), Direction::Horizontal))
                .collect::<Option<Vec<_>>>()?;
            stack(rows, Direction::Vertical)
        }
    }
}

/// `first` takes 60% along `main`; the rest are stacked along `others`.
fn main_split(
    first: &str,
    rest: &[String],
    main: Direction,
    others: Direction,
) -> Option<LayoutNode> {
    let head = LayoutNode::leaf(first.to_string());
    if rest.is_empty() {
        return Some(head);
    }
    let tail = stack(
        rest.iter().map(|n| LayoutNode::leaf(n.clone())).collect(),
        others,
    )?;
    Some(LayoutNode::Split {
        direction: main,
        ratio: 0.6,
        left: Box::new(head),
        right: Box::new(tail),
    })
}

/// Splits `nodes` evenly along `dir`.
fn stack(mut nodes: Vec<LayoutNode>, dir: Direction) -> Option<LayoutNode> {
    if nodes.len() <= 1 {
        return nodes.pop();
    }
    let first = nodes.remove(0);
    let ratio = 1.0 / (nodes.len() + 1) as f32;
    let rest = stack(nodes, dir)?;
    Some(LayoutNode::Split {
        direction: dir,
        ratio,
        left: Box::new(first),
        right: Box::new(rest),
    })
}
