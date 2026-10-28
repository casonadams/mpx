use std::collections::HashMap;

use mpx::layout::Rect;
use mpx::tui::panes::{PaneRect, neighbor_pane, pending_resizes, wrap_step};

fn pane(name: &str, width: u16, height: u16) -> PaneRect {
    (name.to_string(), Rect::new(0, 0, width, height), false)
}

#[test]
fn resize_is_pending_for_new_panes_and_changed_sizes_only() {
    let panes = vec![
        pane("new", 80, 24),
        pane("same", 40, 10),
        pane("grew", 100, 30),
    ];
    let mut dims = HashMap::new();
    dims.insert("same".to_string(), (40, 10));
    dims.insert("grew".to_string(), (40, 10));
    assert_eq!(
        pending_resizes(&panes, &dims),
        vec![("new".to_string(), 80, 24), ("grew".to_string(), 100, 30)]
    );
}

#[test]
fn zero_sized_panes_never_need_a_resize() {
    let panes = vec![pane("flat", 0, 24), pane("thin", 80, 0)];
    assert!(pending_resizes(&panes, &HashMap::new()).is_empty());
}

#[test]
fn neighbor_pane_wraps_around_in_both_directions() {
    let panes = vec![pane("a", 10, 10), pane("b", 10, 10), pane("c", 10, 10)];
    assert_eq!(neighbor_pane(&panes, "c", true), Some("a".to_string()));
    assert_eq!(neighbor_pane(&panes, "a", false), Some("c".to_string()));
    assert_eq!(neighbor_pane(&panes, "a", true), Some("b".to_string()));
}

#[test]
fn neighbor_pane_needs_at_least_two_panes() {
    let panes = vec![pane("solo", 10, 10)];
    assert_eq!(neighbor_pane(&panes, "solo", true), None);
    assert_eq!(neighbor_pane(&panes, "missing", true), None);
}

#[test]
fn wrap_step_moves_one_place_and_wraps_at_both_ends() {
    assert_eq!(wrap_step(0, 3, true), 1);
    assert_eq!(wrap_step(2, 3, true), 0);
    assert_eq!(wrap_step(0, 3, false), 2);
    assert_eq!(wrap_step(1, 3, false), 0);
}

#[test]
fn a_lone_or_unknown_pane_has_no_neighbor() {
    let lone: Vec<PaneRect> = vec![("a".to_string(), mpx::layout::Rect::new(0, 0, 10, 5), true)];
    assert_eq!(neighbor_pane(&lone, "a", true), None);
    assert_eq!(neighbor_pane(&lone, "zz", true), None);
}
