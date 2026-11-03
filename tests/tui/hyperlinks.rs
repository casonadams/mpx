use mpx::tui::canvas::Canvas;
use mpx::vt::{Cell, CellStyle, Color, LinkId, Underline};

fn cell(c: char, style: CellStyle) -> Cell {
    Cell { c, style }
}

/// Draws one row of `cells` at the top-left of a 10x3 canvas and returns what was sent.
fn draw(cells: &[Cell]) -> String {
    let mut canvas = Canvas::default();
    canvas.begin(10, 3, None);
    let mut out = Vec::new();
    canvas.put_row(&mut out, 0, 0, cells).unwrap();
    canvas.finish(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_hyperlink_is_opened_before_its_text_and_closed_after_it() {
    let link = LinkId::intern(";https://canvas.test/a").unwrap();
    let linked = CellStyle {
        link: Some(link),
        ..CellStyle::default()
    };
    let out = draw(&[cell('a', linked), cell('b', linked), cell('c', CellStyle::default())]);
    let opened = "\x1b]8;;https://canvas.test/a\x1b\\";
    let open = out.find(opened).expect("link opened");
    let rest = &out[open + opened.len()..];
    let text = rest.find("ab").expect("linked text drawn");
    let close = rest.find("\x1b]8;;\x1b\\").expect("link closed");
    assert!(text + 2 <= close, "the link closes after its text: {out:?}");
    assert!(rest[close..].contains('c'), "unlinked text follows: {out:?}");
}

#[test]
fn an_underline_style_and_colour_reach_the_outer_terminal() {
    let style = CellStyle {
        underline: Underline::Curly,
        ..CellStyle::default()
    };
    let out = draw(&[cell('w', style)]);
    assert!(out.contains("4:3"), "curly underline selected: {out:?}");
}


#[test]
fn an_underline_colour_is_sent_in_the_colon_form() {
    let style = CellStyle {
        underline: Underline::Single,
        underline_color: Some(Color::Rgb(1, 2, 3)),
        ..CellStyle::default()
    };
    let out = draw(&[cell('w', style)]);
    assert!(out.contains("58:2::1:2:3"), "{out:?}");
}