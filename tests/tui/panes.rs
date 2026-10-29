use std::collections::HashMap;

use mpx::layout::{Divider, Rect};
use mpx::protocol::{NodeInfo, Side};
use mpx::tui::panes::{
    BorderGrab, PaneRect, border_at, border_move, neighbor_pane, pending_resizes,
    resized_elsewhere, wrap_step,
};

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

/// `left │ right` over a 21x10 area, with the right column split into `top ─ bottom`.
fn grid_with_borders() -> (Vec<Divider>, Vec<PaneRect>) {
    let at = |name: &str, rect| (name.to_string(), rect, false);
    let dividers = vec![
        Divider {
            is_vertical: true,
            x: 10,
            y: 0,
            length: 10,
        },
        Divider {
            is_vertical: false,
            x: 11,
            y: 4,
            length: 10,
        },
    ];
    let panes = vec![
        at("left", Rect::new(0, 0, 10, 10)),
        at("top", Rect::new(11, 0, 10, 4)),
        at("bottom", Rect::new(11, 5, 10, 5)),
    ];
    (dividers, panes)
}

#[test]
fn a_border_is_grabbed_with_the_panes_on_either_side_of_the_pointer() {
    let (dividers, panes) = grid_with_borders();
    let grab = |col, row| border_at(&dividers, &panes, (col, row));
    let between = |vertical, before: &str, after: &str| BorderGrab {
        vertical,
        before: before.into(),
        after: after.into(),
    };
    assert_eq!(grab(10, 2), Some(between(true, "left", "top")));
    assert_eq!(grab(10, 7), Some(between(true, "left", "bottom")));
    assert_eq!(grab(15, 4), Some(between(false, "top", "bottom")));
    assert_eq!(grab(5, 5), None, "inside a pane");
}

#[test]
fn dragging_a_border_grows_the_pane_the_pointer_moves_toward() {
    let (dividers, panes) = grid_with_borders();
    let column = border_at(&dividers, &panes, (10, 2)).unwrap();
    assert_eq!(
        border_move(&column, &panes, (13, 2)),
        Some(("left".into(), Side::Right, 3))
    );
    assert_eq!(
        border_move(&column, &panes, (8, 9)),
        Some(("top".into(), Side::Left, 2))
    );
    assert_eq!(border_move(&column, &panes, (10, 0)), None, "already there");
    let row = border_at(&dividers, &panes, (15, 4)).unwrap();
    assert_eq!(
        border_move(&row, &panes, (15, 6)),
        Some(("top".into(), Side::Down, 2))
    );
    assert_eq!(
        border_move(&row, &panes, (15, 1)),
        Some(("bottom".into(), Side::Up, 3))
    );
}

#[test]
fn panes_resized_by_something_else_are_found_by_their_reported_size() {
    let node = |name: &str, size| {
        let mut info = NodeInfo::new(name.to_string(), true, "sh".into());
        info.size = size;
        info
    };
    let nodes = [
        node("kept", Some((80, 24))),
        node("changed", Some((50, 10))),
        node("unknown", None),
        node("never-sent", Some((10, 10))),
    ];
    let sent: HashMap<String, (u16, u16)> = [
        ("kept", (80, 24)),
        ("changed", (80, 24)),
        ("unknown", (80, 24)),
    ]
    .into_iter()
    .map(|(name, size)| (name.to_string(), size))
    .collect();
    assert_eq!(resized_elsewhere(&sent, &nodes), ["changed"]);
}
