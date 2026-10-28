use mpx::layout::Rect;
use mpx::protocol::{NodeInfo, NodeState};
use mpx::tui::app::Mode;
use mpx::tui::copy::{CopyState, SelectionKind};
use mpx::tui::frame::{drawable, pane_view};
use mpx::tui::panes::PaneRect;
use mpx::vt::grid::Grid;

fn pane(name: &str, width: u16, height: u16, focused: bool) -> PaneRect {
    (name.to_string(), Rect::new(0, 0, width, height), focused)
}

fn exited(name: &str, code: Option<i32>) -> NodeInfo {
    let mut node = NodeInfo::new(name.to_string(), true, "sh".to_string());
    node.state = NodeState::Exited;
    node.exit_code = code;
    node
}

fn copy_at(scroll: usize) -> Mode {
    Mode::Copy(CopyState {
        cursor_x: 0,
        cursor_y: 0,
        scroll_offset: scroll,
        anchor_x: 0,
        anchor_abs_y: 0,
        mode: SelectionKind::Line,
        search_prompt: None,
        last_search: None,
    })
}

#[test]
fn focused_pane_in_copy_mode_shows_the_copy_badge_and_scroll() {
    let grid = Grid::new(10, 5);
    let view = pane_view(
        &copy_at(3),
        &grid,
        &[],
        &pane("main", 10, 5, true),
        (1, false, true),
    );
    let badge = view.badge.expect("copy badge");
    assert!(badge.contains("VISUAL LINE"), "{badge}");
    assert!(badge.contains("-3/"), "{badge}");
    assert_eq!(view.scroll_offset, 3);
    assert!(view.selection.is_some());
}

#[test]
fn unfocused_pane_ignores_copy_mode() {
    let grid = Grid::new(10, 5);
    let view = pane_view(
        &copy_at(3),
        &grid,
        &[],
        &pane("other", 10, 5, false),
        (2, false, true),
    );
    assert_eq!(view.badge, None);
    assert_eq!(view.scroll_offset, 0);
    assert_eq!(view.selection, None);
}

#[test]
fn exited_pane_shows_its_exit_code() {
    let grid = Grid::new(10, 5);
    let nodes = vec![exited("done", Some(2)), exited("gone", None)];
    let view = pane_view(
        &Mode::Normal,
        &grid,
        &nodes,
        &pane("done", 10, 5, false),
        (1, false, true),
    );
    assert_eq!(view.badge.as_deref(), Some("[exited 2]"));
    let view = pane_view(
        &Mode::Normal,
        &grid,
        &nodes,
        &pane("gone", 10, 5, false),
        (1, false, true),
    );
    assert_eq!(view.badge.as_deref(), Some("[exited]"));
}

#[test]
fn running_pane_without_copy_mode_has_no_badge() {
    let grid = Grid::new(10, 5);
    let nodes = vec![NodeInfo::new("live".to_string(), true, "sh".to_string())];
    let view = pane_view(
        &Mode::Normal,
        &grid,
        &nodes,
        &pane("live", 10, 5, true),
        (1, false, true),
    );
    assert_eq!(view.badge, None);
}

#[test]
fn unfocused_panes_dim_only_when_several_are_shown_and_none_is_zoomed() {
    let grid = Grid::new(10, 5);
    let idle = pane("idle", 10, 5, false);
    let focus = pane("focus", 10, 5, true);
    assert!(pane_view(&Mode::Normal, &grid, &[], &idle, (2, false, true)).dimmed);
    assert!(!pane_view(&Mode::Normal, &grid, &[], &focus, (2, false, true)).dimmed);
    assert!(!pane_view(&Mode::Normal, &grid, &[], &idle, (1, false, true)).dimmed);
    assert!(!pane_view(&Mode::Normal, &grid, &[], &idle, (2, true, true)).dimmed);
    assert!(!pane_view(&Mode::Normal, &grid, &[], &idle, (2, false, false)).dimmed);
}

#[test]
fn zero_sized_panes_are_not_drawn() {
    let panes = vec![
        pane("a", 10, 5, true),
        pane("flat", 0, 5, false),
        pane("thin", 10, 0, false),
    ];
    let names: Vec<String> = drawable(&panes)
        .into_iter()
        .map(|(name, ..)| name)
        .collect();
    assert_eq!(names, vec!["a".to_string()]);
}
