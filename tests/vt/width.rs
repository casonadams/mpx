use mpx::vt::VtScreen;
use mpx::vt::capture::{CaptureSpec, capture_text};

fn screen(cols: usize, rows: usize, input: &str) -> VtScreen {
    let mut vt = VtScreen::new(cols, rows);
    vt.feed(input.as_bytes());
    vt
}

fn rows(vt: &VtScreen) -> Vec<String> {
    let grid = vt.grid();
    (0..grid.rows)
        .map(|y| grid.line_text(grid.history_len() + y).unwrap())
        .collect()
}

fn cursor(vt: &VtScreen) -> (usize, usize) {
    let grid = vt.grid();
    (grid.cursor.x.min(grid.cols - 1), grid.cursor.y)
}

#[test]
fn characters_take_their_unicode_width() {
    // (what, cols, input, first row, cursor)
    let cases: &[(&str, usize, &str, &str, (usize, usize))] = &[
        (
            "CJK takes two columns",
            20,
            "a\u{4e2d}\u{6587}b",
            "a\u{4e2d}\u{6587}b",
            (6, 0),
        ),
        (
            "emoji takes two columns",
            20,
            "a\u{1f600}b",
            "a\u{1f600}b",
            (4, 0),
        ),
        (
            "fullwidth Latin takes two columns",
            20,
            "\u{ff21}c",
            "\u{ff21}c",
            (3, 0),
        ),
        (
            "a combining mark joins the cell before it",
            20,
            "e\u{301}x",
            "e\u{301}x",
            (2, 0),
        ),
        (
            "a combining mark joins a wide character",
            20,
            "\u{4e2d}\u{301}x",
            "\u{4e2d}\u{301}x",
            (3, 0),
        ),
        (
            "a combining mark with no cell before it is dropped",
            20,
            "\u{301}x",
            "x",
            (1, 0),
        ),
        (
            "ZWJ joins, the next emoji takes its own columns",
            20,
            "a\u{1f468}\u{200d}\u{1f469}b",
            "a\u{1f468}\u{200d}\u{1f469}b",
            (6, 0),
        ),
        (
            "a zero-width space takes no column",
            20,
            "a\u{200b}b",
            "a\u{200b}b",
            (2, 0),
        ),
        ("a C1 control is not printed", 20, "a\u{85}b", "ab", (2, 0)),
        (
            "REP repeats a wide character",
            20,
            "\u{4e2d}\x1b[2b",
            "\u{4e2d}\u{4e2d}\u{4e2d}",
            (6, 0),
        ),
        (
            "writing over the right half blanks the left",
            20,
            "\u{4e2d}\u{6587}\x1b[1;2HX",
            " X\u{6587}",
            (2, 0),
        ),
        (
            "writing over the left half blanks the right",
            20,
            "\u{4e2d}\u{6587}\x1b[1;1HX",
            "X \u{6587}",
            (1, 0),
        ),
        (
            "a wide character over narrow ones",
            20,
            "abcd\x1b[1;2H\u{4e2d}",
            "a\u{4e2d}d",
            (3, 0),
        ),
        (
            "a wide character straddling two wide ones",
            20,
            "\u{4e2d}\u{6587}\x1b[1;2H\u{5b57}",
            " \u{5b57}",
            (3, 0),
        ),
        (
            "insert mode shifts by two columns",
            20,
            "ab\x1b[1;1H\x1b[4h\u{4e2d}",
            "\u{4e2d}ab",
            (2, 0),
        ),
        (
            "EL from the right half erases the whole character",
            20,
            "\u{4e2d}\u{6587}x\x1b[1;2H\x1b[K",
            "",
            (1, 0),
        ),
        (
            "EL 1 to the left half erases the whole character",
            20,
            "\u{4e2d}\u{6587}x\x1b[1;3H\x1b[1K",
            "    x",
            (2, 0),
        ),
        (
            "ECH over a right half erases the whole character",
            20,
            "\u{4e2d}\u{6587}x\x1b[1;2H\x1b[X",
            "  \u{6587}x",
            (1, 0),
        ),
        (
            "DCH leaving a lone right half blanks it",
            20,
            "\u{4e2d}\u{6587}x\x1b[1;1H\x1b[P",
            " \u{6587}x",
            (0, 0),
        ),
        (
            "ICH pushing a right half off the row blanks the left",
            5,
            "abc\u{4e2d}\x1b[1;1H\x1b[@",
            " abc",
            (0, 0),
        ),
        (
            "emoji presentation makes a narrow character wide",
            20,
            "a\u{2764}\u{fe0f}b",
            "a\u{2764}\u{fe0f}b",
            (4, 0),
        ),
        (
            "a keycap takes two columns",
            20,
            "#\u{fe0f}\u{20e3}x",
            "#\u{fe0f}\u{20e3}x",
            (3, 0),
        ),
        (
            "emoji presentation leaves a wide character two columns",
            20,
            "\u{1f600}\u{fe0f}x",
            "\u{1f600}\u{fe0f}x",
            (3, 0),
        ),
        (
            "emoji presentation pushes the cell after it",
            20,
            "abc\x1b[1;1H\u{2764}\u{fe0f}",
            "\u{2764}\u{fe0f}c",
            (2, 0),
        ),
        (
            "emoji presentation with no room left keeps the character narrow",
            3,
            "ab\u{2764}\u{fe0f}",
            "ab\u{2764}",
            (2, 0),
        ),
        (
            "writing over the right half of a widened character blanks it",
            20,
            "\u{2764}\u{fe0f}x\x1b[1;2HX",
            " Xx",
            (2, 0),
        ),
        (
            "writing over a widened ASCII character blanks its right half",
            20,
            "#\u{fe0f}x\x1b[1;1HX",
            "X x",
            (1, 0),
        ),
    ];
    for &(what, cols, input, want, at) in cases {
        let vt = screen(cols, 3, input);
        assert_eq!(rows(&vt)[0], want, "{what}: row");
        assert_eq!(cursor(&vt), at, "{what}: cursor");
    }
}

#[test]
fn a_wide_character_that_does_not_fit_wraps_whole() {
    let vt = screen(5, 3, "abcd\u{4e2d}e");
    assert_eq!(rows(&vt), ["abcd", "\u{4e2d}e", ""]);
    assert_eq!(cursor(&vt), (3, 1));
    assert!(vt.grid().line_wrapped(0));
}

#[test]
fn a_combining_mark_at_a_pending_wrap_joins_the_last_cell() {
    let vt = screen(3, 2, "abc\u{301}");
    assert_eq!(rows(&vt), ["abc\u{301}", ""]);
    assert_eq!(vt.grid().get_cell(2, 0).unwrap().c, 'c');
}

#[test]
fn narrowing_the_screen_through_a_wide_character_blanks_it() {
    let mut vt = screen(4, 2, "ab\u{4e2d}");
    vt.grid_mut().resize(3, 2);
    assert_eq!(rows(&vt), ["ab", ""]);
}

#[test]
fn history_keeps_wide_characters_and_combining_marks() {
    let vt = screen(6, 2, "\u{4e2d}e\u{301}\r\n2\r\n3");
    assert_eq!(vt.grid().history_len(), 1);
    assert_eq!(vt.grid().line_text(0).unwrap(), "\u{4e2d}e\u{301}");
}

#[test]
fn capture_writes_each_character_once() {
    let vt = screen(10, 1, "a\u{4e2d}e\u{301}");
    assert_eq!(vt.scrollback_text(None), "a\u{4e2d}e\u{301}");
    let plain = capture_text(vt.grid(), &CaptureSpec::default());
    assert_eq!(plain, "a\u{4e2d}e\u{301}\n");
    let styled = CaptureSpec {
        escapes: true,
        ..CaptureSpec::default()
    };
    assert_eq!(capture_text(vt.grid(), &styled), "a\u{4e2d}e\u{301}\n");
}
