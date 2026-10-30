use mpx::config::BorderStyle;
use mpx::layout::Rect;
use mpx::protocol::{PopupAnchor, PopupSize, PopupSpec};
use mpx::tui::popup::{inner_rect, popup_rect};

fn spec(width: PopupSize, height: PopupSize, x: PopupAnchor, y: PopupAnchor) -> PopupSpec {
    PopupSpec {
        width,
        height,
        x,
        y,
        ..PopupSpec::default()
    }
}

/// The pane area of a 100 x 30 terminal whose status row is the top one.
fn screen() -> Rect {
    Rect::new(0, 1, 100, 30)
}

#[test]
fn the_default_popup_is_half_the_screen_centred() {
    let rect = popup_rect(&PopupSpec::default(), screen(), None);
    assert_eq!(rect, Rect::new(25, 8, 50, 15));
}

#[test]
fn cells_and_percentages_size_the_popup() {
    let s = spec(
        PopupSize::Cells(40),
        PopupSize::Percent(10),
        PopupAnchor::Start,
        PopupAnchor::Start,
    );
    assert_eq!(popup_rect(&s, screen(), None), Rect::new(0, 1, 40, 3));
}

#[test]
fn edges_and_cells_place_the_popup() {
    let right_bottom = spec(
        PopupSize::Cells(10),
        PopupSize::Cells(5),
        PopupAnchor::End,
        PopupAnchor::End,
    );
    assert_eq!(
        popup_rect(&right_bottom, screen(), None),
        Rect::new(90, 26, 10, 5)
    );
    let offset = spec(
        PopupSize::Cells(10),
        PopupSize::Cells(5),
        PopupAnchor::Cells(7),
        PopupAnchor::Cells(2),
    );
    assert_eq!(popup_rect(&offset, screen(), None), Rect::new(7, 3, 10, 5));
}

#[test]
fn pane_position_follows_the_active_pane() {
    let s = spec(
        PopupSize::Cells(20),
        PopupSize::Cells(4),
        PopupAnchor::Pane,
        PopupAnchor::Pane,
    );
    let pane = Rect::new(40, 10, 30, 10);
    assert_eq!(
        popup_rect(&s, screen(), Some(pane)),
        Rect::new(40, 10, 20, 4)
    );
    assert_eq!(popup_rect(&s, screen(), None), Rect::new(0, 1, 20, 4));
}

#[test]
fn a_popup_larger_than_the_screen_is_kept_on_it() {
    let s = spec(
        PopupSize::Cells(500),
        PopupSize::Percent(200),
        PopupAnchor::Cells(90),
        PopupAnchor::Cells(20),
    );
    let rect = popup_rect(&s, screen(), None);
    assert_eq!(rect.width, 100);
    assert_eq!(rect.height, 30);
    assert_eq!((rect.x, rect.y), (0, 1));
}

#[test]
fn the_program_screen_is_the_popup_less_its_border() {
    let outer = Rect::new(25, 8, 50, 15);
    assert_eq!(inner_rect(outer, true), Rect::new(26, 9, 48, 13));
    assert_eq!(inner_rect(outer, false), outer);
    assert_eq!(
        inner_rect(Rect::new(0, 0, 2, 2), true),
        Rect::new(1, 1, 1, 1)
    );
}

#[test]
fn border_lines_default_to_the_pane_style() {
    assert_eq!(PopupSpec::default().lines, None);
    assert_eq!(BorderStyle::default(), BorderStyle::Single);
}
