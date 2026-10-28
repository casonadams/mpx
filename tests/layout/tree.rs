use mpx::layout::tree::{Direction, preset_tree};
use mpx::layout::{LayoutNode, Rect};
use mpx::protocol::{LayoutPreset, Side};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn rects_of(node: &LayoutNode, bounds: Rect) -> Vec<(String, Rect)> {
    node.compute_rects(bounds)
}

fn width_of(rects: &[(String, Rect)], name: &str) -> u16 {
    rects
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| r.width)
        .unwrap()
}

fn height_of(rects: &[(String, Rect)], name: &str) -> u16 {
    rects
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| r.height)
        .unwrap()
}

const VIEW: Rect = Rect {
    x: 0,
    y: 0,
    width: 80,
    height: 24,
};

#[test]
fn even_horizontal_splits_width_evenly() {
    let tree = preset_tree(LayoutPreset::EvenHorizontal, &names(&["a", "b", "c"])).unwrap();
    let rects = rects_of(&tree, VIEW);
    for name in ["a", "b", "c"] {
        assert!((25..=27).contains(&width_of(&rects, name)), "{name} width");
        assert_eq!(height_of(&rects, name), 24);
    }
}

#[test]
fn even_vertical_splits_height_evenly() {
    let tree = preset_tree(LayoutPreset::EvenVertical, &names(&["a", "b"])).unwrap();
    let rects = rects_of(&tree, VIEW);
    assert_eq!(width_of(&rects, "a"), 80);
    assert_eq!(height_of(&rects, "a") + height_of(&rects, "b") + 1, 24);
    assert!((11..=12).contains(&height_of(&rects, "a")));
}

#[test]
fn main_vertical_gives_first_pane_sixty_percent_width() {
    let tree = preset_tree(LayoutPreset::MainVertical, &names(&["main", "x", "y"])).unwrap();
    let rects = rects_of(&tree, VIEW);
    assert!((46..=48).contains(&width_of(&rects, "main")));
    assert_eq!(height_of(&rects, "x") + height_of(&rects, "y") + 1, 24);
}

#[test]
fn main_horizontal_gives_first_pane_sixty_percent_height() {
    let tree = preset_tree(LayoutPreset::MainHorizontal, &names(&["main", "x", "y"])).unwrap();
    let rects = rects_of(&tree, VIEW);
    assert!((13..=15).contains(&height_of(&rects, "main")));
    assert_eq!(width_of(&rects, "main"), 80);
}

#[test]
fn tiled_arranges_a_grid() {
    let tree = preset_tree(LayoutPreset::Tiled, &names(&["a", "b", "c", "d"])).unwrap();
    let rects = rects_of(&tree, VIEW);
    assert_eq!(rects.len(), 4);
    let mut origins: Vec<(u16, u16)> = rects.iter().map(|(_, r)| (r.x, r.y)).collect();
    origins.sort();
    origins.dedup();
    assert_eq!(origins.len(), 4, "four distinct cells");
    assert!((39..=40).contains(&width_of(&rects, "a")));
}

#[test]
fn preset_needs_panes() {
    assert!(preset_tree(LayoutPreset::Tiled, &[]).is_none());
}

#[test]
fn split_with_ratio_gives_new_pane_its_share() {
    let mut node = LayoutNode::leaf("a".to_string());
    assert!(node.split_leaf_with("a", "b".to_string(), Direction::Horizontal, 0.75));
    let rects = rects_of(&node, VIEW);
    assert_eq!(width_of(&rects, "a"), 59);
    assert_eq!(width_of(&rects, "b"), 20);
}

#[test]
fn split_unknown_target_changes_nothing() {
    let mut node = LayoutNode::leaf("a".to_string());
    assert!(!node.split_leaf("zzz", "b".to_string(), Direction::Vertical));
    assert_eq!(node.all_leaves(), vec!["a".to_string()]);
}

#[test]
fn resize_moves_the_nearest_border_towards_the_side() {
    let mut node = LayoutNode::leaf("a".to_string());
    node.split_leaf("a", "b".to_string(), Direction::Horizontal);
    assert!(node.resize_border("a", Side::Right, 10, VIEW));
    assert_eq!(width_of(&rects_of(&node, VIEW), "a"), 50);

    assert!(node.resize_border("b", Side::Left, 15, VIEW));
    assert_eq!(width_of(&rects_of(&node, VIEW), "a"), 35);
}

#[test]
fn resize_clamps_the_ratio() {
    let mut node = LayoutNode::leaf("a".to_string());
    node.split_leaf("a", "b".to_string(), Direction::Horizontal);
    assert!(node.resize_border("a", Side::Right, 500, VIEW));
    let a = width_of(&rects_of(&node, VIEW), "a");
    assert!((70..=71).contains(&a), "ratio clamped to 0.9, got {a}");
}

#[test]
fn resize_without_matching_border_is_refused() {
    let mut node = LayoutNode::leaf("a".to_string());
    node.split_leaf("a", "b".to_string(), Direction::Horizontal);
    assert!(!node.resize_border("a", Side::Down, 3, VIEW));
    assert!(
        !node.resize_border("a", Side::Left, 3, VIEW),
        "a is leftmost"
    );
}

#[test]
fn map_leaves_rewrites_names() {
    let mut node = LayoutNode::leaf("a".to_string());
    node.split_leaf("a", "b".to_string(), Direction::Vertical);
    node.map_leaves(&mut |n: &mut String| n.push('!'));
    assert_eq!(node.all_leaves(), vec!["a!".to_string(), "b!".to_string()]);
    assert!(node.has_leaf("b!"));
    assert!(!node.has_leaf("b"));
}
