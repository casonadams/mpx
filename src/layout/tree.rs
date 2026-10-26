use serde::{Deserialize, Serialize};

use super::geometry::Rect;

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
                let (r1, r2) = match direction {
                    Direction::Horizontal => {
                        let (l, r) = bounds.split_horizontal(*ratio);
                        dividers.push(Divider {
                            is_vertical: true,
                            x: l.x + l.width,
                            y: bounds.y,
                            length: bounds.height,
                        });
                        (l, r)
                    }
                    Direction::Vertical => {
                        let (t, b) = bounds.split_vertical(*ratio);
                        dividers.push(Divider {
                            is_vertical: false,
                            x: bounds.x,
                            y: t.y + t.height,
                            length: bounds.width,
                        });
                        (t, b)
                    }
                };
                left.collect_layout(r1, panes, dividers);
                right.collect_layout(r2, panes, dividers);
            }
        }
    }

    pub fn split_leaf(&mut self, target: &str, new_name: String, dir: Direction) -> bool {
        match self {
            Self::Leaf { name } => {
                if name == target {
                    let old_leaf = LayoutNode::Leaf { name: target.to_string() };
                    let new_leaf = LayoutNode::Leaf { name: new_name };
                    *self = LayoutNode::Split {
                        direction: dir,
                        ratio: 0.5,
                        left: Box::new(old_leaf),
                        right: Box::new(new_leaf),
                    };
                    true
                } else {
                    false
                }
            }
            Self::Split { left, right, .. } => {
                left.split_leaf(target, new_name.clone(), dir)
                    || right.split_leaf(target, new_name, dir)
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
}
