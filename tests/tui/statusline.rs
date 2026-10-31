use std::time::{Duration, Instant};

use mpx::config::Justify;
use mpx::layout::WindowTab;
use mpx::tui::statusline::{
    Item, Line, Seg, Style, Target, compose, parse_styled, rows, segs_width, status_due,
    status_vars, strftime, tab_flags, truncate,
};
use mpx::vt::Color;

/// 2024-03-05 14:07:09 UTC.
const MARCH_5: u64 = 1_709_647_629_000;

fn tab(index: usize, name: &str, active: bool) -> WindowTab {
    WindowTab {
        index,
        name: name.to_string(),
        is_active: active,
        is_last: false,
        has_activity: false,
        has_bell: false,
        has_marked: false,
        marked_pane: None,
        agent_badge: None,
        focused: None,
        zoomed: false,
    }
}

fn item(text: &str, target: Target) -> Item {
    Item {
        segs: vec![Seg::plain(text)],
        target: Some(target),
    }
}

fn text(segs: &[Seg]) -> String {
    segs.iter().map(|s| s.text.as_str()).collect()
}

/// `compose` with plain `left` and `right` text; an empty `right` adds no cells.
fn compose_plain(
    width: usize,
    left: &str,
    items: Vec<Item>,
    separator: &str,
    right: &str,
    justify: Justify,
) -> Line {
    compose(width, vec![Seg::plain(left)], items, separator, vec![Seg::plain(right)], justify)
}

/// Asserts `strftime(format, unix_ms, utc_offset_secs)` for each `(format, unix_ms, offset, want)`.
fn assert_strftime(cases: &[(&str, u64, i64, &str)]) {
    for &(format, unix_ms, offset, want) in cases {
        assert_eq!(strftime(format, unix_ms, offset), want, "format {format:?}");
    }
}

#[test]
fn strftime_expands_the_tmux_status_subset() {
    assert_strftime(&[
        ("%H:%M %d-%b-%y", MARCH_5, 0, "14:07 05-Mar-24"),
        ("%I %p", MARCH_5, 0, "02 PM"),
        ("%e %a %Y %m %S", MARCH_5, 0, " 5 Tue 2024 03 09"),
    ]);
}

#[test]
fn strftime_applies_the_offset_and_prints_it_for_zone() {
    assert_strftime(&[
        ("%H %Z", MARCH_5, 3600, "15 +0100"),
        ("%H:%M %Z", MARCH_5, -5 * 3600, "09:07 -0500"),
    ]);
}

#[test]
fn strftime_maps_midnight_and_noon_to_twelve() {
    const MIDNIGHT: u64 = 1_709_596_800_000;
    assert_strftime(&[
        ("%I:%M %p", MIDNIGHT + 30 * 60 * 1000, 0, "12:30 AM"),
        ("%I %p", MIDNIGHT + 12 * 3600 * 1000, 0, "12 PM"),
    ]);
}

#[test]
fn strftime_keeps_unknown_and_trailing_specifiers() {
    assert_strftime(&[("100%% %Q", MARCH_5, 0, "100% %Q"), ("end%", MARCH_5, 0, "end%")]);
}

#[test]
fn style_directives_colour_the_text_after_them() {
    let segs = parse_styled("#[fg=red,bold]hi#[default]x");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].text, "hi");
    assert_eq!(segs[0].style.fg, Some(Color::Red));
    assert!(segs[0].style.bold);
    assert_eq!(segs[1].text, "x");
    assert_eq!(segs[1].style, Style::default());
}

#[test]
fn hex_background_and_attribute_resets_parse() {
    let segs = parse_styled("#[bg=#112233,underscore]a#[nounderscore]b");
    assert_eq!(segs[0].style.bg, Some(Color::Rgb(0x11, 0x22, 0x33)));
    assert!(segs[0].style.underscore);
    assert!(!segs[1].style.underscore);
    assert_eq!(segs[1].style.bg, Some(Color::Rgb(0x11, 0x22, 0x33)));
}

#[test]
fn unknown_directives_are_ignored_and_unclosed_ones_stay_text() {
    let segs = parse_styled("a#[nope]b");
    assert_eq!(text(&segs), "ab");
    assert!(segs.iter().all(|s| s.style == Style::default()));
    assert_eq!(text(&parse_styled("x#[fg=red")), "x#[fg=red");
    assert_eq!(text(&parse_styled("#[align=left]z")), "z");
}

#[test]
fn compose_lays_items_out_with_separators_and_justification() {
    let cases: [(
        usize,
        &str,
        Vec<Item>,
        &str,
        &str,
        Justify,
        &str,
        Vec<(usize, usize, Target)>,
    ); 2] = [
        (
            20,
            "[dev] ",
            vec![item("0:a* ", Target::Window(0)), item("1:b ", Target::Window(1))],
            "",
            "12:00",
            Justify::Left,
            "[dev] 0:a* 1:b 12:00",
            vec![(6, 11, Target::Window(0)), (11, 15, Target::Window(1))],
        ),
        (
            12,
            "[s] ",
            vec![item("A", Target::Window(0)), item("B", Target::Window(1))],
            " | ",
            "",
            Justify::Left,
            "[s] A | B   ",
            vec![(4, 5, Target::Window(0)), (8, 9, Target::Window(1))],
        ),
    ];
    for (width, left, items, separator, right, justify, want, targets) in cases {
        let line = compose_plain(width, left, items, separator, right, justify);
        assert_eq!(text(&line.segs), want);
        assert_eq!(segs_width(&line.segs), width);
        assert_eq!(line.targets, targets);
    }
}

#[test]
fn compose_centres_and_right_justifies_the_window_list() {
    let items = || vec![item("AB", Target::Window(0))];
    let centred = compose_plain(10, "L", items(), "", "R", Justify::Centre);
    // 10 cells: L and R take 2, the tab 2, so 6 are free: 3 before and 3 after.
    assert_eq!(text(&centred.segs), "L   AB   R");
    assert_eq!(centred.targets, vec![(4, 6, Target::Window(0))]);
    let right = compose_plain(10, "L", items(), "", "R", Justify::Right);
    assert_eq!(text(&right.segs), "L      ABR");
    assert_eq!(right.targets, vec![(7, 9, Target::Window(0))]);
}

#[test]
fn compose_never_pads_past_the_width() {
    let line = compose(
        4,
        vec![Seg::plain("toolong")],
        Vec::new(),
        "",
        vec![Seg::plain("R")],
        Justify::Left,
    );
    assert_eq!(text(&line.segs), "toolongR");
}

#[test]
fn truncate_keeps_the_leading_cells_and_their_style() {
    let segs = parse_styled("#[fg=red]hello world");
    let cut = truncate(segs, 3);
    assert_eq!(text(&cut), "hel");
    assert_eq!(cut[0].style.fg, Some(Color::Red));
}

#[test]
fn rows_put_the_status_line_at_the_bottom_or_top() {
    let bottom = rows(24, true, false);
    assert_eq!(
        (bottom.status, bottom.prompt, bottom.pane_top, bottom.pane_h),
        (Some(23), 23, 0, 23)
    );
    let top = rows(24, true, true);
    assert_eq!(
        (top.status, top.prompt, top.pane_top, top.pane_h),
        (Some(0), 0, 1, 23)
    );
}

#[test]
fn rows_with_the_status_line_hidden_give_every_row_to_the_panes() {
    let hidden = rows(24, false, false);
    assert_eq!(hidden.status, None);
    assert_eq!((hidden.pane_top, hidden.pane_h), (0, 24));
    assert_eq!(hidden.prompt, 23, "messages still use the bottom row");
}

#[test]
fn status_interval_one_redraws_every_tick_and_zero_never_does() {
    let now = Instant::now();
    assert!(status_due(1, Some(now), now));
    assert!(!status_due(0, None, now));
    assert!(status_due(5, None, now), "the first frame is always due");
}

#[test]
fn longer_status_intervals_wait_for_their_time() {
    let start = Instant::now();
    assert!(!status_due(5, Some(start), start + Duration::from_secs(2)));
    assert!(status_due(5, Some(start), start + Duration::from_secs(5)));
}

#[test]
fn window_flags_follow_tmux_order() {
    let mut active = tab(0, "dev", true);
    active.has_marked = true;
    assert_eq!(tab_flags(&active, true), "*ZM");
    let mut last = tab(1, "build", false);
    last.has_activity = true;
    last.has_bell = true;
    last.is_last = true;
    assert_eq!(tab_flags(&last, false), "#!-");
}

#[test]
fn status_vars_number_the_window_from_base_index() {
    let window = tab(2, "dev", true);
    let vars = status_vars("main", None, Some(&window), false, 1);
    assert_eq!(vars.get("window_index").map(String::as_str), Some("3"));
    assert_eq!(vars.get("window_name").map(String::as_str), Some("dev"));
    assert_eq!(vars.get("window_flags").map(String::as_str), Some("*"));
    assert_eq!(vars.get("session_name").map(String::as_str), Some("main"));
}
