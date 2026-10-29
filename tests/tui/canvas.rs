use mpx::layout::Rect;
use mpx::tui::canvas::{Canvas, write_sgr};
use mpx::vt::{Cell, CellStyle, Color};

fn cell(c: char) -> Cell {
    Cell {
        c,
        style: CellStyle::default(),
    }
}

fn row(text: &str) -> Vec<Cell> {
    text.chars().map(cell).collect()
}

/// Runs one frame over a 10x3 canvas and returns what it sent.
fn frame(
    canvas: &mut Canvas,
    occluded: Option<Rect>,
    draw: impl FnOnce(&mut Canvas, &mut Vec<u8>),
) -> String {
    canvas.begin(10, 3, occluded);
    let mut out = Vec::new();
    draw(canvas, &mut out);
    canvas.finish(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn sgr(style: CellStyle) -> String {
    let mut out = Vec::new();
    write_sgr(&mut out, &style).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_cell_already_on_screen_is_not_sent_again() {
    let mut canvas = Canvas::default();
    let first = frame(&mut canvas, None, |c, out| {
        c.put(out, 3, 1, cell('x')).unwrap()
    });
    assert_eq!(first, "\x1b[2;4H\x1b[0mx");
    let second = frame(&mut canvas, None, |c, out| {
        c.put(out, 3, 1, cell('x')).unwrap()
    });
    assert_eq!(second, "");
}

#[test]
fn colors_keep_their_palette_slot_and_attributes_are_sent() {
    let style = |fg, bg| CellStyle {
        fg: Some(fg),
        bg: Some(bg),
        ..CellStyle::default()
    };
    assert_eq!(sgr(style(Color::Red, Color::Blue)), "\x1b[0;31;44m");
    assert_eq!(
        sgr(style(Color::BrightRed, Color::BrightBlue)),
        "\x1b[0;91;104m"
    );
    assert_eq!(
        sgr(style(Color::Indexed(208), Color::Rgb(1, 2, 3))),
        "\x1b[0;38;5;208;48;2;1;2;3m"
    );
    let attrs = CellStyle {
        bold: true,
        italic: true,
        underline: true,
        ..CellStyle::default()
    };
    assert_eq!(sgr(attrs), "\x1b[0;1;3;4m");
    let reverse = CellStyle {
        reverse: true,
        ..CellStyle::default()
    };
    assert_eq!(sgr(reverse), "\x1b[0;7m");
}

#[test]
fn cells_under_an_overlay_repaint_once_it_closes() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        c.put(out, 1, 1, cell('a')).unwrap()
    });
    let covered = Some(Rect::new(0, 0, 5, 3));
    let hidden = frame(&mut canvas, covered, |c, out| {
        c.put(out, 1, 1, cell('a')).unwrap()
    });
    assert_eq!(hidden, "", "nothing is drawn under the overlay");
    let shown = frame(&mut canvas, None, |c, out| {
        c.put(out, 1, 1, cell('a')).unwrap()
    });
    assert!(shown.ends_with('a'), "{shown:?}");
}

#[test]
fn resize_and_invalidate_repaint_everything() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        c.put(out, 0, 0, cell('a')).unwrap()
    });
    canvas.invalidate();
    let after_invalidate = frame(&mut canvas, None, |c, out| {
        c.put(out, 0, 0, cell('a')).unwrap()
    });
    assert!(after_invalidate.ends_with('a'));

    canvas.begin(12, 3, None);
    let mut out = Vec::new();
    canvas.put(&mut out, 0, 0, cell('a')).unwrap();
    assert!(
        String::from_utf8(out).unwrap().ends_with('a'),
        "a new size repaints"
    );
}

/// What redrawing the top row as `after` sends once it shows `before`.
fn redraw_row(before: &str, after: &str) -> String {
    let mut canvas = Canvas::default();
    let mut sent = String::new();
    for text in [before, after] {
        sent = frame(&mut canvas, None, |c, out| {
            c.put_row(out, 0, 0, &row(text)).unwrap()
        });
    }
    sent
}

#[test]
fn an_ascii_row_sends_only_its_changed_cells() {
    assert_eq!(redraw_row("hello", "hellO"), "\x1b[1;5H\x1b[0mO");
}

#[test]
fn text_over_a_blank_row_goes_out_as_one_run_not_a_jump_per_word() {
    assert_eq!(
        redraw_row("          ", "ab cd  efg"),
        "\x1b[1;1H\x1b[0mab cd  efg"
    );
}

#[test]
fn gaps_longer_than_a_cursor_move_or_in_another_style_are_jumped() {
    let mut canvas = Canvas::default();
    let bold = Cell {
        c: ' ',
        style: CellStyle {
            bold: true,
            ..CellStyle::default()
        },
    };
    frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("          ")).unwrap();
        c.put_row(out, 0, 1, &[cell(' '), bold, cell(' ')]).unwrap();
    });
    let far = frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("a        b")).unwrap()
    });
    assert_eq!(far, "\x1b[1;1H\x1b[0ma\x1b[1;10Hb");
    let styled = frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 1, &[cell('a'), bold, cell('b')]).unwrap()
    });
    assert_eq!(styled, "\x1b[2;1H\x1b[0ma\x1b[2;3Hb");
}

#[test]
fn a_row_with_wide_glyphs_is_reprinted_whole_and_in_sequence() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("漢字 1")).unwrap()
    });
    let sent = frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("漢字 2")).unwrap()
    });
    assert_eq!(
        sent, "\x1b[1;1H\x1b[0m漢字 2",
        "one address, then the row in order"
    );

    let to_ascii = frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("ab   ")).unwrap()
    });
    assert_eq!(
        to_ascii, "\x1b[1;1H\x1b[0mab   ",
        "a row last sent with wide glyphs is reprinted too"
    );
}

#[test]
fn cells_right_of_a_reprinted_wide_row_are_repainted() {
    let mut canvas = Canvas::default();
    let draw = |text: &'static str| {
        move |c: &mut Canvas, out: &mut Vec<u8>| {
            c.put_row(out, 0, 0, &row(text)).unwrap();
            c.put(out, 6, 0, cell('|')).unwrap();
        }
    };
    frame(&mut canvas, None, draw("漢字 1"));
    let sent = frame(&mut canvas, None, draw("漢字 2"));
    assert!(
        sent.ends_with("\x1b[1;7H|"),
        "the neighbor is redrawn: {sent:?}"
    );
}

#[test]
fn the_status_row_is_sent_only_when_it_changes() {
    let mut canvas = Canvas::default();
    canvas.begin(10, 3, None);
    assert!(canvas.status_changed(b"[s] 0:main"));
    assert!(!canvas.status_changed(b"[s] 0:main"));
    assert!(canvas.status_changed(b"[s] 0:work"));
    canvas.invalidate();
    assert!(canvas.status_changed(b"[s] 0:work"));
}

#[test]
fn scrolling_moves_the_shadow_so_moved_rows_are_not_resent() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        for (y, text) in ["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    let sent = frame(&mut canvas, None, |c, out| {
        assert!(c.scroll(out, Rect::new(0, 0, 10, 3), 1).unwrap());
        for (y, text) in ["bbbbbbbbbb", "cccccccccc", "dddddddddd"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    assert!(sent.starts_with("\x1b[0m\x1b[1;3r\x1b[1S\x1b[r"), "{sent:?}");
    assert!(sent.contains("dddddddddd"), "{sent:?}");
    assert!(!sent.contains('b') && !sent.contains('c'), "moved rows resent: {sent:?}");
}

#[test]
fn scrolling_down_leaves_the_top_rows_to_be_painted() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        for (y, text) in ["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    let sent = frame(&mut canvas, None, |c, out| {
        assert!(c.scroll(out, Rect::new(0, 0, 10, 3), -1).unwrap());
        for (y, text) in ["zzzzzzzzzz", "aaaaaaaaaa", "bbbbbbbbbb"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    assert!(sent.contains("\x1b[1T") && sent.contains("zzzzzzzzzz"), "{sent:?}");
    assert!(!sent.contains('a') && !sent.contains('b'), "{sent:?}");
}

#[test]
fn scrolling_is_refused_under_chrome_and_for_degenerate_moves() {
    let mut canvas = Canvas::default();
    let covered = Rect::new(2, 1, 3, 1);
    let sent = frame(&mut canvas, Some(covered), |c, out| {
        assert!(!c.scroll(out, Rect::new(0, 0, 10, 3), 1).unwrap());
        assert!(!c.scroll(out, Rect::new(0, 0, 10, 3), 0).unwrap());
    });
    assert!(sent.is_empty(), "{sent:?}");
    frame(&mut canvas, None, |c, out| {
        assert!(!c.scroll(out, Rect::new(0, 0, 10, 3), 3).unwrap(), "moving a whole region is a repaint");
        assert!(!c.scroll(out, Rect::new(0, 2, 10, 1), 1).unwrap(), "a one-row region cannot scroll");
        assert!(!c.scroll(out, Rect::new(0, 0, 10, 6), 1).unwrap(), "outside the canvas");
    });
}

/// A 10x3 frame with rows `0123456789`, `abcdefghij`, `klmnopqrst`.
fn lettered(canvas: &mut Canvas) {
    frame(canvas, None, |c, out| {
        for (y, text) in ["0123456789", "abcdefghij", "klmnopqrst"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
}

#[test]
fn a_region_beside_the_sidebar_scrolls_inside_left_right_margins() {
    let mut canvas = Canvas::default();
    canvas.set_lr_margins(true);
    lettered(&mut canvas);
    let sent = frame(&mut canvas, None, |c, out| {
        assert!(c.scroll(out, Rect::new(4, 0, 6, 3), 1).unwrap());
        for (y, text) in ["0123efghij", "abcdopqrst", "klmnUVWXYZ"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    assert!(
        sent.starts_with("\x1b[0m\x1b[?69h\x1b[1;3r\x1b[5;10s\x1b[1S\x1b[s\x1b[?69l\x1b[r"),
        "{sent:?}"
    );
    assert!(sent.contains("UVWXYZ"), "{sent:?}");
    for moved in ["efghij", "opqrst", "0123", "abcd", "klmn"] {
        assert!(!sent.contains(moved), "{moved} was resent: {sent:?}");
    }
}

#[test]
fn a_narrow_region_is_repainted_without_margin_support_or_beside_wide_glyphs() {
    let mut canvas = Canvas::default();
    lettered(&mut canvas);
    let refused = frame(&mut canvas, None, |c, out| {
        assert!(!c.scroll(out, Rect::new(4, 0, 6, 3), 1).unwrap(), "no margin support");
    });
    assert!(refused.is_empty());

    let mut canvas = Canvas::default();
    canvas.set_lr_margins(true);
    frame(&mut canvas, None, |c, out| {
        for (y, text) in ["0123456789", "abc漢efghij", "klmnopqrst"].iter().enumerate() {
            c.put_row(out, 0, y as u16, &row(text)).unwrap();
        }
    });
    frame(&mut canvas, None, |c, out| {
        assert!(!c.scroll(out, Rect::new(4, 0, 6, 3), 1).unwrap(), "a wide glyph beside the region");
    });
}

#[test]
fn every_covered_region_is_skipped_and_repainted_once_uncovered() {
    let mut canvas = Canvas::default();
    let full = |c: &mut Canvas, out: &mut Vec<u8>| {
        for y in 0..3 {
            c.put_row(out, 0, y, &row("abcdefghij")).unwrap();
        }
    };
    frame(&mut canvas, None, full);
    canvas.begin(10, 3, [Rect::new(0, 0, 2, 3), Rect::new(6, 1, 3, 1)]);
    let mut covered = Vec::new();
    full(&mut canvas, &mut covered);
    assert!(covered.is_empty(), "nothing drawn under either region");
    let after = frame(&mut canvas, None, full);
    assert_eq!(after.matches("ab").count(), 3, "the left region repainted: {after:?}");
    assert!(after.contains("ghi"), "the popup region repainted: {after:?}");
}
