use mpx::layout::Rect;

/// The (start, length) of `rect` along the split axis.
fn along(rect: &Rect, horizontal: bool) -> (u16, u16) {
    if horizontal {
        (rect.x, rect.width)
    } else {
        (rect.y, rect.height)
    }
}

fn split(bounds: &Rect, ratio: f32, horizontal: bool) -> (Rect, Rect) {
    if horizontal {
        bounds.split_horizontal(ratio)
    } else {
        bounds.split_vertical(ratio)
    }
}

/// Both sides of the split lie inside the parent, one divider cell apart, and a side keeps a cell
/// whenever the parent has room for two.
fn assert_split_fits(bounds: Rect, ratio: f32, horizontal: bool) {
    let (first, second) = split(&bounds, ratio, horizontal);
    let (start, len) = along(&bounds, horizontal);
    let (first_at, first_len) = along(&first, horizontal);
    let (second_at, second_len) = along(&second, horizontal);
    let case = format!("{bounds:?} ratio {ratio} horizontal {horizontal}");
    assert_eq!(first_at, start, "{case}");
    assert_eq!(first_len + second_len + 1, len, "{case}");
    assert!(
        second_at < start + len,
        "{case}: second side starts past the edge"
    );
    assert!(second_at + second_len <= start + len, "{case}");
    if len >= 3 {
        assert!(first_len >= 1 && second_len >= 1, "{case}");
    }
}

#[test]
fn narrow_and_short_splits_stay_inside_their_parent() {
    for horizontal in [true, false] {
        for len in 1..=3 {
            let bounds = if horizontal {
                Rect::new(5, 2, len, 4)
            } else {
                Rect::new(5, 2, 4, len)
            };
            for ratio in [0.1, 0.5, 0.9] {
                assert_split_fits(bounds, ratio, horizontal);
            }
        }
    }
}

#[test]
fn extreme_ratios_leave_both_sides_a_cell_once_there_is_room() {
    for horizontal in [true, false] {
        for len in 3..=40 {
            let bounds = if horizontal {
                Rect::new(0, 0, len, 1)
            } else {
                Rect::new(0, 0, 1, len)
            };
            for ratio in [0.1, 0.9] {
                assert_split_fits(bounds, ratio, horizontal);
            }
        }
    }
}

#[test]
fn ratios_outside_the_range_are_clamped() {
    let bounds = Rect::new(0, 0, 80, 24);
    assert_eq!(bounds.split_horizontal(-5.0), bounds.split_horizontal(0.1));
    assert_eq!(bounds.split_horizontal(5.0), bounds.split_horizontal(0.9));
    assert_eq!(bounds.split_vertical(-5.0), bounds.split_vertical(0.1));
    assert_eq!(bounds.split_vertical(5.0), bounds.split_vertical(0.9));
}

#[test]
fn a_zero_sized_parent_yields_zero_sized_sides() {
    let (left, right) = Rect::new(3, 3, 0, 0).split_horizontal(0.5);
    assert_eq!((left.width, right.width), (0, 0));
    let (top, bottom) = Rect::new(3, 3, 0, 0).split_vertical(0.5);
    assert_eq!((top.height, bottom.height), (0, 0));
}
