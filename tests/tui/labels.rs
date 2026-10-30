use mpx::layout::Rect;
use mpx::protocol::NodeInfo;
use mpx::tui::labels::{clock_lines, label, label_rect, pane_numbers};

fn node(name: &str, window: usize) -> NodeInfo {
    let mut n = NodeInfo::new(name.to_string(), true, "sh".to_string());
    n.window = Some(window);
    n
}

fn pane(name: &str) -> (String, Rect, bool) {
    (name.to_string(), Rect::new(0, 0, 10, 5), false)
}

#[test]
fn clock_is_block_digits_centered_in_the_area() {
    let lines = clock_lines(9, 41, 40, 7);
    assert_eq!(lines.len(), 7);
    assert!(lines.iter().all(|l| l.chars().count() == 40));
    assert!(lines[0].trim().is_empty(), "one blank row above the digits");
    assert!(lines[6].trim().is_empty(), "one blank row below the digits");
    for row in &lines[1..6] {
        assert!(row.contains('█'), "digit rows are drawn: {row:?}");
    }
    let first = lines[1].chars().take_while(|c| *c == ' ').count();
    assert!(first > 0, "the block is centered, not left-aligned");
}

#[test]
fn clock_uses_narrow_digits_then_plain_text_as_room_shrinks() {
    // Wide digits need 6 columns a glyph, narrow ones 3: 09:41 is 38 and 19 columns.
    let narrow = clock_lines(9, 41, 20, 5);
    assert_eq!(narrow[0].chars().count(), 20);
    assert!(narrow.iter().any(|row| row.contains('█')));

    let plain = clock_lines(9, 41, 10, 5);
    assert_eq!(plain[2].trim(), "09:41", "plain text on the middle row");
    assert!(plain.iter().all(|row| !row.contains('█')));
}

#[test]
fn clock_is_empty_without_room() {
    assert!(clock_lines(1, 2, 0, 5).is_empty());
    assert!(clock_lines(1, 2, 10, 0).is_empty());
}

#[test]
fn clock_pads_hours_and_minutes_to_two_digits() {
    assert_eq!(clock_lines(0, 5, 5, 1), vec!["00:05".to_string()]);
}

#[test]
fn label_is_block_digits_when_they_fit_and_plain_text_otherwise() {
    let big = label("7", 10, 10);
    assert_eq!(big.len(), 5);
    assert_eq!(big[0].chars().count(), 6);

    let narrow = label("7", 3, 5);
    assert_eq!(narrow.len(), 5);
    assert_eq!(narrow[0].chars().count(), 3);

    assert_eq!(
        label("12", 3, 5),
        vec!["12".to_string()],
        "too wide for blocks"
    );
    assert_eq!(
        label("12", 10, 2),
        vec!["12".to_string()],
        "too short for blocks"
    );
    assert_eq!(
        label("x", 10, 10),
        vec!["x".to_string()],
        "no glyph, plain text"
    );
    assert!(label("", 10, 10).is_empty());
    assert!(label("7", 0, 5).is_empty());
}

#[test]
fn label_rect_is_centered_in_its_pane() {
    let rows = label("7", 10, 10);
    let at = label_rect(Rect::new(0, 0, 10, 10), &rows);
    assert_eq!(at, Rect::new(2, 2, 6, 5));
}

#[test]
fn pane_numbers_follow_node_order_within_their_window() {
    let nodes = [node("a", 0), node("b", 1), node("c", 0)];
    // Panes arrive in layout order, which need not be node order.
    let panes = [pane("c"), pane("a")];
    assert_eq!(
        pane_numbers(&panes, &nodes),
        vec![(1, "c".to_string()), (0, "a".to_string())]
    );
}

#[test]
fn pane_numbers_fall_back_to_layout_position_for_unknown_panes() {
    let panes = [pane("ghost"), pane("other")];
    assert_eq!(
        pane_numbers(&panes, &[]),
        vec![(0, "ghost".to_string()), (1, "other".to_string())]
    );
}
