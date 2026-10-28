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

#[test]
fn an_ascii_row_sends_only_its_changed_cells() {
    let mut canvas = Canvas::default();
    frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("hello")).unwrap()
    });
    let sent = frame(&mut canvas, None, |c, out| {
        c.put_row(out, 0, 0, &row("hellO")).unwrap()
    });
    assert_eq!(sent, "\x1b[1;5H\x1b[0mO");
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
