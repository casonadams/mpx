//! A keyframe fed to a fresh screen reproduces the grid it was taken from.
use mpx::vt::keyframe::keyframe;
use mpx::vt::{Cell, VtScreen};

const COLS: usize = 40;
const ROWS: usize = 12;

fn lines(count: usize) -> String {
    (0..count)
        .map(|i| format!("line {i:02} \x1b[3{}mcolor\x1b[0m\r\n", i % 8))
        .collect()
}

/// Output of different programs, each leaving the screen in a state a keyframe must carry.
fn corpus() -> Vec<(&'static str, String)> {
    vec![
        ("scrolled shell", lines(30)),
        ("wrapped long line", format!("{}\r\nnext", "w".repeat(COLS * 2 + 7))),
        (
            "alternate screen app",
            "\x1b[?1049h\x1b[2J\x1b[44;3m status \x1b[0m\x1b[3;1Hbody\x1b[7m rev \x1b[K\x1b[0m".to_string(),
        ),
        (
            "scroll region and saved cursor",
            "\x1b[2;9r\x1b[9;1H1\r\n2\r\n3\x1b[5;7H\x1b7\x1b[8;3Htext".to_string(),
        ),
        (
            "styles, links and wide text",
            concat!(
                "\x1b]8;;https://k.test/a\x1b\\LINK\x1b]8;;\x1b\\ \x1b[4:3mcurly\x1b[0m ",
                "\x1b[58;2;1;2;3m\x1b[4mred\x1b[0m \x1b[38;5;208mo\x1b[38;2;9;8;7mrgb\x1b[0m café → ✓ 日本",
            )
            .to_string(),
        ),
        ("pending wrap at the margin", "x".repeat(COLS)),
        ("hidden cursor and style left on", "\x1b[?25l\x1b[1;31mbold red".to_string()),
        (
            "left and right margins",
            "\x1b[?69h\x1b[5;20s\x1b[3;8Hinside".to_string(),
        ),
        ("bottom row filled", format!("{}x", "\r\n".repeat(ROWS - 1))),
    ]
}

fn rows(screen: &VtScreen) -> Vec<Vec<Cell>> {
    screen.grid().screen_lines().map(<[Cell]>::to_vec).collect()
}

#[test]
fn a_keyframe_rebuilds_the_grid_whatever_output_made_it() {
    for (what, output) in corpus() {
        let mut source = VtScreen::live(COLS, ROWS);
        source.feed(output.as_bytes());
        let frame = keyframe(source.grid());

        let mut rebuilt = VtScreen::live(COLS, ROWS);
        rebuilt.feed(frame.as_bytes());
        let (a, b) = (source.grid(), rebuilt.grid());

        assert_eq!(rows(&rebuilt), rows(&source), "{what}: cells");
        assert_eq!(b.cursor, a.cursor, "{what}: cursor");
        assert_eq!(b.saved_cursor, a.saved_cursor, "{what}: saved cursor");
        assert_eq!(
            (
                b.scroll_top,
                b.scroll_bottom,
                b.lr_margins,
                b.scroll_left,
                b.scroll_right
            ),
            (
                a.scroll_top,
                a.scroll_bottom,
                a.lr_margins,
                a.scroll_left,
                a.scroll_right
            ),
            "{what}: margins"
        );
        assert_eq!(b.current_style, a.current_style, "{what}: pen");
        assert_eq!(
            b.alternate_screen(),
            a.alternate_screen(),
            "{what}: alternate screen"
        );
        for y in 0..ROWS - 1 {
            assert_eq!(b.line_wrapped(y), a.line_wrapped(y), "{what}: row {y} wrap");
        }
    }
}

#[test]
fn text_after_a_keyframe_lands_where_it_would_have() {
    for (what, output) in corpus() {
        let mut live = VtScreen::live(COLS, ROWS);
        live.feed(output.as_bytes());
        let mut rebuilt = VtScreen::live(COLS, ROWS);
        rebuilt.feed(keyframe(live.grid()).as_bytes());

        let more = "\r\nAFTER-KEYFRAME \x1b[2;3Hmid\x1b8back";
        live.feed(more.as_bytes());
        rebuilt.feed(more.as_bytes());
        assert_eq!(rows(&rebuilt), rows(&live), "{what}");
    }
}
