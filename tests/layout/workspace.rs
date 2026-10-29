use mpx::layout::tree::Direction;
use mpx::layout::{Rect, Workspace};
use mpx::protocol::{LayoutPreset, Side};

fn main_tab_ws() -> Workspace {
    let mut ws = Workspace::new("main".to_string());
    ws.add_or_split("pane1".to_string(), Direction::Horizontal);
    ws
}

fn widths(ws: &Workspace) -> Vec<(String, u16, bool)> {
    ws.compute_layout(80, 24)
        .into_iter()
        .map(|(n, r, f)| (n, r.width, f))
        .collect()
}

#[test]
fn single_pane_fills_the_viewport() {
    let ws = main_tab_ws();
    let layout = ws.compute_layout(100, 50);
    assert_eq!(layout.len(), 1);
    let (name, rect, focused) = &layout[0];
    assert_eq!(name, "pane1");
    assert_eq!(*rect, Rect::new(0, 0, 100, 50));
    assert!(focused);
}

#[test]
fn split_focuses_the_new_pane() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    let layout = ws.compute_layout(100, 50);
    let p1 = layout.iter().find(|(n, ..)| n == "pane1").unwrap();
    let p2 = layout.iter().find(|(n, ..)| n == "pane2").unwrap();
    assert_eq!(p1.1, Rect::new(0, 0, 50, 50));
    assert_eq!(p2.1, Rect::new(51, 0, 49, 50));
    assert!(p2.2);
}

#[test]
fn split_percent_gives_new_pane_that_share() {
    let mut ws = main_tab_ws();
    ws.split_pane(
        Some("pane1"),
        "pane2".to_string(),
        Direction::Horizontal,
        Some(25),
        false,
    )
    .unwrap();
    let w = widths(&ws);
    let p2 = w.iter().find(|(n, ..)| n == "pane2").unwrap();
    assert_eq!(p2.1, 20);
    assert!(!p2.2, "focus false keeps focus on pane1");
}

#[test]
fn split_unknown_target_is_an_error() {
    let mut ws = main_tab_ws();
    assert!(
        ws.split_pane(
            Some("nope"),
            "x".to_string(),
            Direction::Vertical,
            None,
            true
        )
        .is_err()
    );
}

#[test]
fn new_window_without_focus_keeps_active_window() {
    let mut ws = main_tab_ws();
    ws.new_window_with_focus("second".into(), "s".into(), false);
    assert_eq!(ws.active_idx, 0);
    ws.new_window_with_focus("third".into(), "t".into(), true);
    assert_eq!(ws.active_idx, 2);
}

#[test]
fn window_cycling_and_last_window_toggle() {
    let mut ws = Workspace::new("main".into());
    ws.new_window("w0".into(), "a".into());
    ws.new_window("w1".into(), "b".into());
    assert_eq!(ws.active_idx, 1);

    assert!(ws.last_window_switch());
    assert_eq!(ws.active_idx, 0);
    assert!(ws.last_window_switch());
    assert_eq!(ws.active_idx, 1);

    ws.next_window();
    assert_eq!(ws.active_idx, 0);
    ws.prev_window();
    assert_eq!(ws.active_idx, 1);
}

#[test]
fn last_window_is_refused_without_history() {
    let mut ws = Workspace::new("main".into());
    ws.new_window("w0".into(), "a".into());
    assert!(!ws.last_window_switch());
}

#[test]
fn focus_last_returns_to_previous_pane() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    assert!(ws.focus_last());
    let focused: Vec<String> = widths(&ws)
        .into_iter()
        .filter(|w| w.2)
        .map(|w| w.0)
        .collect();
    assert_eq!(focused, vec!["pane1".to_string()]);
    assert!(ws.focus_last(), "toggles back");
}

#[test]
fn renamed_window_keeps_its_name_against_auto_naming() {
    let mut ws = Workspace::new("main".into());
    ws.new_window("label".into(), "a".into());
    ws.rename_window(0, "work".into());
    ws.set_auto_name(0, Some("claude".into()));
    assert_eq!(ws.window_tabs()[0].name, "work");
}

#[test]
fn auto_name_falls_back_to_the_label() {
    let mut ws = Workspace::new("main".into());
    ws.new_window("label".into(), "a".into());
    ws.set_auto_name(0, Some("zsh".into()));
    assert_eq!(ws.window_tabs()[0].name, "zsh");
    ws.set_auto_name(0, None);
    assert_eq!(ws.window_tabs()[0].name, "label");
}

#[test]
fn rename_out_of_range_is_refused() {
    let mut ws = Workspace::new("main".into());
    assert!(!ws.rename_window(3, "x".into()));
}

#[test]
fn swap_exchanges_panes_across_windows() {
    let mut ws = main_tab_ws();
    ws.new_window("other".into(), "c".into());
    assert!(ws.swap("pane1", "c"));
    assert_eq!(ws.window_of("c"), Some(0));
    assert_eq!(ws.window_of("pane1"), Some(1));
    assert!(!ws.swap("pane1", "missing"));
}

#[test]
fn mark_pane_toggles_and_clears_and_updates_tabs() {
    let mut ws = main_tab_ws();
    ws.new_window("other".into(), "c".into());

    // Initially no mark
    assert_eq!(ws.marked_pane(), None);
    assert!(!ws.window_tabs()[0].has_marked);
    assert!(!ws.window_tabs()[1].has_marked);

    // Mark pane1 in window 0
    assert!(ws.mark_pane("pane1"));
    assert_eq!(ws.marked_pane(), Some("pane1"));
    assert!(ws.window_tabs()[0].has_marked);
    assert_eq!(ws.window_tabs()[0].marked_pane.as_deref(), Some("pane1"));
    assert!(!ws.window_tabs()[1].has_marked);

    // Marking same pane unmarks it
    assert!(!ws.mark_pane("pane1"));
    assert_eq!(ws.marked_pane(), None);
    assert!(!ws.window_tabs()[0].has_marked);

    // Mark pane in window 1
    assert!(ws.mark_pane("c"));
    assert_eq!(ws.marked_pane(), Some("c"));
    assert!(!ws.window_tabs()[0].has_marked);
    assert!(ws.window_tabs()[1].has_marked);

    // Marking nonexistent pane fails
    assert!(!ws.mark_pane("nonexistent"));
    // Old mark is preserved
    assert_eq!(ws.marked_pane(), Some("c"));

    // Clear mark
    ws.clear_mark_pane();
    assert_eq!(ws.marked_pane(), None);
}

#[test]
fn marked_pane_lifecycle_swap_and_removal() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    ws.mark_pane("pane2");
    assert_eq!(ws.marked_pane(), Some("pane2"));

    // Swap panes: pane2 is still the marked pane
    assert!(ws.swap("pane1", "pane2"));
    assert_eq!(ws.marked_pane(), Some("pane2"));

    // Removing pane2 clears the mark
    ws.remove("pane2");
    assert_eq!(ws.marked_pane(), None);

    // Creating another window, marking its pane, and killing the window clears mark
    let win2 = ws.new_window("win2".into(), "leaf".into());
    ws.mark_pane("leaf");
    assert_eq!(ws.marked_pane(), Some("leaf"));
    ws.kill_window(win2);
    assert_eq!(ws.marked_pane(), None);
}

#[test]
fn break_pane_moves_it_into_its_own_window() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    assert!(ws.break_pane("pane2"));
    assert_eq!(ws.windows.len(), 2);
    assert_eq!(ws.active_idx, 1);
    assert_eq!(ws.window_of("pane2"), Some(1));
    assert_eq!(ws.window_of("pane1"), Some(0));
}

#[test]
fn break_refuses_a_lone_pane() {
    let mut ws = main_tab_ws();
    assert!(!ws.break_pane("pane1"));
}

#[test]
fn kill_window_returns_its_panes_and_clamps_active() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    ws.new_window("second".into(), "s".into());
    assert_eq!(ws.active_idx, 1);
    let panes = ws.kill_window(1).unwrap();
    assert_eq!(panes, vec!["s".to_string()]);
    assert_eq!(ws.windows.len(), 1);
    assert_eq!(ws.active_idx, 0);
    assert!(ws.kill_window(5).is_none());
}

#[test]
fn select_layout_cycles_when_no_preset_given() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    assert!(ws.select_layout(None));
    let first: Vec<u16> = widths(&ws).iter().map(|w| w.1).collect();
    assert_eq!(first.iter().sum::<u16>() + 1, 80, "even-horizontal");

    assert!(ws.select_layout(None));
    let vertical: Vec<u16> = widths(&ws).iter().map(|w| w.1).collect();
    assert!(
        vertical.iter().all(|w| *w == 80),
        "even-vertical stacks full width"
    );

    assert!(ws.select_layout(Some(LayoutPreset::MainVertical)));
    let main = widths(&ws);
    assert!((46..=48).contains(&main[0].1));
}

#[test]
fn resize_split_moves_the_border_and_reports_missing_borders() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    assert!(ws.resize_split("pane1", Side::Right, 10, 80, 24));
    let w = widths(&ws);
    assert_eq!(w[0].1, 50);
    assert!(ws.resize_split("pane2", Side::Left, 5, 80, 24));
    assert_eq!(widths(&ws)[0].1, 45);
    assert!(!ws.resize_split("pane1", Side::Up, 2, 80, 24));
    assert!(!ws.resize_split("ghost", Side::Right, 2, 80, 24));
}

#[test]
fn removing_a_pane_drops_empty_windows() {
    let mut ws = main_tab_ws();
    ws.new_window("second".into(), "s".into());
    ws.remove("s");
    assert_eq!(ws.windows.len(), 1);
    assert!(ws.active_idx < ws.windows.len());
}

#[test]
fn pane_count_spans_windows() {
    let mut ws = main_tab_ws();
    ws.add_or_split("pane2".to_string(), Direction::Horizontal);
    ws.new_window("second".into(), "s".into());
    assert_eq!(ws.pane_count(), 3);
}
