use mpx::vt::VtScreen;
use mpx::vt::capture::{CaptureLine, CaptureSpec, capture_text};

fn screen(cols: usize, rows: usize, bytes: &str) -> VtScreen {
    let mut vt = VtScreen::new(cols, rows);
    vt.feed(bytes.as_bytes());
    vt
}

fn text(vt: &VtScreen, spec: CaptureSpec) -> String {
    capture_text(vt.grid(), &spec)
}

#[test]
fn the_visible_screen_is_captured_by_default_without_trailing_blanks() {
    let vt = screen(6, 3, "ab  \r\ncd");
    assert_eq!(text(&vt, CaptureSpec::default()), "ab\ncd\n\n");
}

#[test]
fn history_comes_before_the_screen_and_negative_lines_count_back_into_it() {
    let vt = screen(5, 2, "one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(vt.grid().history_len(), 2);
    assert_eq!(text(&vt, CaptureSpec::default()), "three\nfour\n");

    let whole = CaptureSpec {
        start: Some(CaptureLine::First),
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, whole), "one\ntwo\nthree\nfour\n");

    let back = CaptureSpec {
        start: Some(CaptureLine::Line(-2)),
        end: Some(CaptureLine::Line(-1)),
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, back), "one\ntwo\n");

    let across = CaptureSpec {
        start: Some(CaptureLine::Line(-1)),
        end: Some(CaptureLine::Line(0)),
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, across), "two\nthree\n");
}

#[test]
fn line_numbers_past_the_history_clamp_to_its_start_and_past_the_screen_to_its_end() {
    let vt = screen(5, 2, "one\r\ntwo\r\nthree\r\nfour");
    let far_back = CaptureSpec {
        start: Some(CaptureLine::Line(-100)),
        end: Some(CaptureLine::Line(100)),
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, far_back), "one\ntwo\nthree\nfour\n");
    let to_last = CaptureSpec {
        start: Some(CaptureLine::Line(1)),
        end: Some(CaptureLine::Last),
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, to_last), "four\n");
}

#[test]
fn a_wrapped_line_is_one_line_with_join_and_its_trailing_spaces_survive() {
    let vt = screen(4, 3, "abcdefg\r\nhi  ");
    assert_eq!(text(&vt, CaptureSpec::default()), "abcd\nefg\nhi\n");
    let joined = CaptureSpec {
        join: true,
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, joined), "abcdefg\nhi\n");

    let vt = screen(4, 2, "ab  cd");
    let joined = CaptureSpec {
        join: true,
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, joined), "ab  cd\n");
}

#[test]
fn a_line_that_wrapped_into_history_still_joins_with_its_continuation() {
    let vt = screen(4, 2, "abcdefghi");
    assert_eq!(vt.grid().history_len(), 1);
    let joined = CaptureSpec {
        start: Some(CaptureLine::First),
        join: true,
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, joined), "abcdefghi\n");
}

#[test]
fn escapes_keep_the_colors_of_the_cells_and_reset_at_the_end_of_the_line() {
    let vt = screen(4, 1, "\x1b[31mred\x1b[0m");
    let escaped = CaptureSpec {
        escapes: true,
        ..CaptureSpec::default()
    };
    assert_eq!(text(&vt, escaped), "\x1b[0;31mred\x1b[0m\n");
    assert_eq!(text(&vt, CaptureSpec::default()), "red\n");
}
