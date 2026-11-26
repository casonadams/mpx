use mpx::vt::VtScreen;
use mpx::vt::grid::{CellStyle, Color};

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
fn control_sequences_match_xterm() {
    // (what, cols, rows, input, screen rows, cursor)
    let cases: &[(&str, usize, usize, &str, &[&str], (usize, usize))] = &[
        (
            "ED 2 keeps the cursor",
            10,
            3,
            "AAAA\r\nBBBB\x1b[2J",
            &["", "", ""],
            (4, 1),
        ),
        ("DEL is ignored", 10, 1, "a\x7fb", &["ab"], (2, 0)),
        (
            "IND moves down a row",
            10,
            3,
            "A\x1bDB",
            &["A", " B", ""],
            (2, 1),
        ),
        (
            "IND scrolls at the bottom margin",
            10,
            2,
            "A\r\nB\x1bDC",
            &["B", " C"],
            (2, 1),
        ),
        (
            "NEL moves to the next row's start",
            10,
            3,
            "A\x1bEB",
            &["A", "B", ""],
            (1, 1),
        ),
        (
            "NEL scrolls at the bottom margin",
            10,
            2,
            "A\r\nB\x1bEC",
            &["B", "C"],
            (1, 1),
        ),
        (
            "DECALN fills the screen with E",
            3,
            2,
            "x\x1b#8",
            &["EEE", "EEE"],
            (0, 0),
        ),
        (
            "BS from a pending wrap steps back from the last column",
            5,
            2,
            "abcde\x08X",
            &["abcXe", ""],
            (4, 0),
        ),
        (
            "CUB from a pending wrap steps back from the last column",
            5,
            2,
            "abcde\x1b[DX",
            &["abcXe", ""],
            (4, 0),
        ),
        (
            "CUB 2 from a pending wrap",
            5,
            2,
            "abcde\x1b[2DX",
            &["abXde", ""],
            (3, 0),
        ),
        (
            "HTS sets a tab stop",
            20,
            1,
            "\x1b[1;4H\x1bH\r\tX",
            &["   X"],
            (4, 0),
        ),
        (
            "TBC 0 clears the stop at the cursor",
            20,
            1,
            "\x1b[1;9H\x1b[g\r\tX",
            &["                X"],
            (17, 0),
        ),
        (
            "TBC 3 clears every stop",
            20,
            1,
            "\x1b[3g\tX",
            &["                   X"],
            (19, 0),
        ),
        (
            "HT stops at the last column without stops",
            12,
            1,
            "\t\t\t\tX",
            &["           X"],
            (11, 0),
        ),
        (
            "CHT moves forward n tab stops",
            40,
            1,
            "\x1b[2IX",
            &["                X"],
            (17, 0),
        ),
        (
            "CHT defaults to one stop",
            40,
            1,
            "a\x1b[IX",
            &["a       X"],
            (9, 0),
        ),
        (
            "CBT honours custom stops",
            20,
            1,
            "\x1b[3g\x1b[1;4H\x1bH\x1b[1;15H\x1b[ZX",
            &["   X"],
            (4, 0),
        ),
        (
            "CBT stops at column 0",
            20,
            1,
            "\x1b[1;5H\x1b[3ZX",
            &["X"],
            (1, 0),
        ),
        (
            "RIS restores default tab stops",
            20,
            1,
            "\x1b[3g\x1bc\tX",
            &["        X"],
            (9, 0),
        ),
    ];
    for &(what, cols, height, input, want, at) in cases {
        let vt = screen(cols, height, input);
        assert_eq!(rows(&vt), want, "{what}: screen");
        assert_eq!(cursor(&vt), at, "{what}: cursor");
    }
}

#[test]
fn decsc_saves_and_decrc_restores_rendition() {
    for (what, save, restore) in [
        ("DECSC", "\x1b7", "\x1b8"),
        ("SCOSC", "\x1b[s", "\x1b[u"),
        ("mode 1048", "\x1b[?1048h", "\x1b[?1048l"),
    ] {
        let vt = screen(10, 2, &format!("\x1b[1;31m{save}\x1b[0;42m{restore}X"));
        let style = vt.grid().get_cell(0, 0).unwrap().style;
        let want = CellStyle {
            bold: true,
            fg: Some(Color::Red),
            ..CellStyle::default()
        };
        assert_eq!(style, want, "{what}");
    }
}

#[test]
fn decrc_restores_origin_mode_and_charset() {
    let vt = screen(
        10,
        6,
        "\x1b[3;5r\x1b[?6h\x1b(0\x1b7\x1b[?6l\x1b(B\x1b8\x1b[1;1Hq",
    );
    assert!(vt.grid().origin_mode);
    assert_eq!(cursor(&vt), (1, 2));
    assert_eq!(vt.grid().get_cell(0, 2).unwrap().c, '\u{2500}');
}

#[test]
fn decrc_without_decsc_homes_and_resets_rendition() {
    let vt = screen(10, 2, "\x1b[2;5H\x1b[1m\x1b8X");
    assert_eq!(cursor(&vt), (1, 0));
    assert_eq!(
        vt.grid().get_cell(0, 0).unwrap().style,
        CellStyle::default()
    );
}
