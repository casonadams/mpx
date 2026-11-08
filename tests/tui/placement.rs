use mpx::layout::geometry::Rect;
use mpx::tui::placement::{Extent, Offset};
use mpx::tui::popup::{PopupOpts, inner_size, popup_rect};

#[test]
fn extents_are_cells_or_percentages() {
    assert_eq!(Extent::parse("40"), Ok(Extent::Cells(40)));
    assert_eq!(Extent::parse("80%"), Ok(Extent::Percent(80)));
    assert!(Extent::parse("101%").is_err(), "more than the screen");
    assert!(Extent::parse("wide").is_err());
}

#[test]
fn an_extent_is_never_less_than_one_cell() {
    assert_eq!(Extent::Cells(0).cells(10), 1);
    assert_eq!(Extent::Percent(1).cells(10), 1);
    assert_eq!(Extent::Percent(50).cells(81), 40);
}

#[test]
fn offsets_are_cells_percentages_centred_or_far() {
    assert_eq!(Offset::parse("C"), Ok(Offset::Centre));
    assert_eq!(Offset::parse("R"), Ok(Offset::Far));
    assert_eq!(Offset::parse("12"), Ok(Offset::Cells(12)));
    assert_eq!(Offset::parse("50%"), Ok(Offset::Percent(50)));
    assert!(Offset::parse("left").is_err());
}

#[test]
fn a_box_stays_on_the_screen_whatever_the_offset() {
    assert_eq!(Offset::Cells(90).start(100, 20), 80);
    assert_eq!(Offset::Percent(100).start(100, 20), 80);
    assert_eq!(Offset::Centre.start(100, 20), 40);
    assert_eq!(Offset::Far.start(100, 20), 80);
    assert_eq!(Offset::Cells(5).start(100, 20), 5);
}

#[test]
fn a_popup_defaults_to_eighty_percent_centred() {
    let rect = popup_rect(&PopupOpts::default(), (100, 30));
    assert_eq!(rect, Rect::new(10, 3, 80, 24));
    assert_eq!(
        inner_size(rect),
        (78, 22),
        "the box's border is not part of the command's area"
    );
}

#[test]
fn a_popup_larger_than_the_screen_is_cut_to_it() {
    let opts = PopupOpts {
        width: Extent::Cells(500),
        height: Extent::Cells(500),
        ..PopupOpts::default()
    };
    assert_eq!(popup_rect(&opts, (100, 30)), Rect::new(0, 0, 100, 30));
}
