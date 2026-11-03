use mpx::keys::{TermModes, paste_buffer_bytes};

#[test]
fn line_feeds_are_carriage_returns_unless_raw_or_a_separator_replaces_them() {
    let plain = TermModes::default();
    assert_eq!(
        paste_buffer_bytes("a\nb\r\nc", None, false, false, &plain),
        b"a\rb\rc"
    );
    assert_eq!(
        paste_buffer_bytes("a\nb\r\nc", None, true, false, &plain),
        b"a\nb\r\nc"
    );
    assert_eq!(
        paste_buffer_bytes("a\nb\r\nc", Some(", "), false, false, &plain),
        b"a, b, c"
    );
}

#[test]
fn a_paste_is_bracketed_when_asked_or_when_the_program_enabled_it() {
    let plain = TermModes::default();
    assert_eq!(
        paste_buffer_bytes("x", None, false, true, &plain),
        b"\x1b[200~x\x1b[201~"
    );
    assert_eq!(paste_buffer_bytes("x", None, false, false, &plain), b"x");
    let bracketed = TermModes {
        bracketed_paste: true,
        ..TermModes::default()
    };
    assert_eq!(
        paste_buffer_bytes("x\ny", None, false, false, &bracketed),
        b"\x1b[200~x\ry\x1b[201~"
    );
}

#[test]
fn an_end_marker_inside_the_text_cannot_end_the_paste_early() {
    let out = paste_buffer_bytes("a\x1b[201~b", None, false, true, &TermModes::default());
    assert_eq!(out, b"\x1b[200~ab\x1b[201~");
}
