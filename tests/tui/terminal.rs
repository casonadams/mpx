use mpx::tui::terminal::{margins_reply, osc_color_reply, parse_color_spec, report_from};

#[test]
fn a_terminal_reporting_mode_69_supports_margins() {
    for ps in ["1", "2", "3"] {
        let reply = format!("\x1b[?69;{ps}$y\x1b[?62;22c");
        assert_eq!(margins_reply(reply.as_bytes()), Some(true), "Ps {ps}");
    }
}

#[test]
fn an_unknown_or_unanswered_mode_means_no_margins() {
    assert_eq!(margins_reply(b"\x1b[?69;0$y\x1b[?62c"), Some(false));
    assert_eq!(margins_reply(b"\x1b[?69;4$y\x1b[?1;2c"), Some(false), "permanently reset");
    assert_eq!(margins_reply(b"\x1b[?65;1;9c"), Some(false), "only DA1 came back");
}

#[test]
fn the_answer_waits_for_device_attributes() {
    assert_eq!(margins_reply(b""), None);
    assert_eq!(margins_reply(b"\x1b[?69;1$y"), None);
    assert_eq!(margins_reply(b"\x1b[?69;1$y\x1b[?62;22"), None, "DA1 cut short");
}

#[test]
fn terminal_color_specs_scale_every_digit_width_to_eight_bits() {
    assert_eq!(parse_color_spec("rgb:ffff/8080/0000"), Some((255, 128, 0)));
    assert_eq!(parse_color_spec("rgb:ff/80/00"), Some((255, 128, 0)));
    assert_eq!(parse_color_spec("rgb:f/8/0"), Some((255, 136, 0)));
    assert_eq!(parse_color_spec("rgb:2e2e/3434/4040"), Some((46, 52, 64)));
    assert_eq!(parse_color_spec("#2e3440"), Some((46, 52, 64)));
    for bad in ["", "rgb:", "rgb:ff/ff", "rgb:ff/ff/ff/ff", "rgb:fffff/0/0", "rgb:zz/0/0", "#fff", "red"] {
        assert_eq!(parse_color_spec(bad), None, "{bad}");
    }
}

#[test]
fn color_answers_are_found_among_the_other_replies_whatever_ends_them() {
    let reply = b"\x1b]10;rgb:d8d8/dede/e9e9\x1b\\\x1b]11;rgb:2e2e/3434/4040\x07\x1b[?69;2$y\x1b[?62c";
    let report = report_from(reply);
    assert_eq!(report.fg, Some((216, 222, 233)));
    assert_eq!(report.bg, Some((46, 52, 64)));
    assert!(report.margins);

    let silent = report_from(b"\x1b[?62c");
    assert_eq!((silent.fg, silent.bg, silent.margins), (None, None, false));
    assert_eq!(osc_color_reply(b"\x1b]11;rgb:0000/0000/0000", 11), None, "unterminated");
    assert_eq!(osc_color_reply(b"\x1b]10;rgb:ffff/ffff/ffff\x07", 11), None, "other code");
}
