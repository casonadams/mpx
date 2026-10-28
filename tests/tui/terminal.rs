use mpx::tui::terminal::margins_reply;

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
