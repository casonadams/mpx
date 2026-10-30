use mpx::tui::render::draw_status_segments;
use mpx::tui::statusline::{Seg, Style};
use mpx::vt::Color;

fn draw(segs: &[Seg]) -> Vec<u8> {
    let mut out = Vec::new();
    draw_status_segments(&mut out, segs, 3, Some(Color::Green), Some(Color::Black)).unwrap();
    out
}

#[test]
fn a_plain_status_row_is_the_built_in_sequence() {
    // Cursor to row 4 column 1, colours from the status row, the text, then a reset.
    assert_eq!(
        draw(&[Seg::plain("HELLO")]),
        b"\x1b[4;1H\x1b[0;30;42mHELLO\x1b[0m"
    );
}

#[test]
fn neighbouring_segments_with_one_style_share_a_colour_sequence() {
    let segs = [Seg::plain("ab"), Seg::plain("cd")];
    assert_eq!(draw(&segs), b"\x1b[4;1H\x1b[0;30;42mabcd\x1b[0m");
}

#[test]
fn a_bold_segment_adds_the_bold_attribute_after_its_colours() {
    let bold = Seg {
        text: "B".into(),
        style: Style {
            bold: true,
            ..Style::default()
        },
    };
    let out = String::from_utf8(draw(&[bold])).unwrap();
    assert!(out.contains("\x1b[0;30;42m\x1b[1mB"), "{out:?}");
}

#[test]
fn a_segment_colour_replaces_the_row_colour_for_its_text() {
    let red = Seg {
        text: "R".into(),
        style: Style {
            fg: Some(Color::Red),
            ..Style::default()
        },
    };
    let out = String::from_utf8(draw(&[red])).unwrap();
    assert!(
        out.contains("\x1b[0;31;42mR"),
        "red text on the row background: {out:?}"
    );
}
