use mpx::vt::VtScreen;

fn rows_after(bytes: &str) -> Vec<String> {
    let mut vt = VtScreen::new(12, 3);
    vt.feed(bytes.as_bytes());
    vt.snapshot()
        .lines()
        .map(|l| l.trim_end().to_owned())
        .collect()
}

#[test]
fn dec_special_graphics_in_g0_draws_lines_until_ascii_is_designated() {
    let rows = rows_after("\x1b(0lqqk\x1b(B ok\r\n\x1b(0x\x1b(Bab\x1b(0x\r\n\x1b(0mqqj");
    assert_eq!(rows[0], "┌──┐ ok");
    assert_eq!(rows[1], "│ab│");
    assert_eq!(rows[2], "└──┘");
}

#[test]
fn shift_out_uses_g1_and_shift_in_returns_to_g0() {
    let rows = rows_after("\x1b)0a\x0eqx\x0fq");
    assert_eq!(rows[0], "a─│q");
}

#[test]
fn repeat_repeats_the_translated_glyph() {
    let rows = rows_after("\x1b(0q\x1b[3b");
    assert_eq!(rows[0], "────");
}

#[test]
fn full_reset_returns_to_ascii() {
    let rows = rows_after("\x1b(0\x1bcq");
    assert_eq!(rows[0], "q");
}
