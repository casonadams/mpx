use mpx::buffer::RingBuffer;
use mpx::node::pane::{LogQuery, render_logs};
use mpx::vt::VtScreen;

const COLORED: &[u8] = b"one\r\n\x1b[31mtwo\x1b[0m\r\nthree\r\n";

fn ring() -> RingBuffer {
    let mut ring = RingBuffer::new(1024);
    ring.write(COLORED);
    ring
}

fn screen() -> VtScreen {
    let mut vt = VtScreen::new(20, 5);
    vt.feed(COLORED);
    vt
}

fn query(tail: Option<usize>, plain: bool, since: Option<u64>) -> LogQuery {
    LogQuery { tail, plain, since }
}

#[test]
fn raw_history_keeps_escapes_and_points_past_the_end() {
    let (text, next) = render_logs(&ring(), Some(&screen()), &query(None, false, None));
    assert_eq!(text.as_bytes(), COLORED);
    assert_eq!(next, COLORED.len() as u64);
}

#[test]
fn raw_tail_keeps_only_the_last_lines() {
    let (text, _) = render_logs(&ring(), None, &query(Some(1), false, None));
    assert_eq!(text, "three\r\n");
}

#[test]
fn plain_history_of_a_terminal_is_the_rendered_scrollback() {
    let vt = screen();
    let (text, next) = render_logs(&ring(), Some(&vt), &query(Some(2), true, None));
    assert_eq!(text, "two\nthree");
    assert_eq!(next, COLORED.len() as u64);
}

#[test]
fn plain_history_without_a_terminal_is_the_raw_output() {
    let (text, _) = render_logs(&ring(), None, &query(None, true, None));
    assert_eq!(text.as_bytes(), COLORED);
}

#[test]
fn since_returns_only_the_delta_and_its_end_offset() {
    let start = "one\r\n".len() as u64;
    let (text, next) = render_logs(&ring(), None, &query(None, false, Some(start)));
    assert_eq!(text.as_bytes(), &COLORED[start as usize..]);
    assert_eq!(next, COLORED.len() as u64);
}

#[test]
fn plain_delta_of_a_terminal_strips_escapes() {
    let start = "one\r\n".len() as u64;
    let vt = screen();
    let (text, _) = render_logs(&ring(), Some(&vt), &query(None, true, Some(start)));
    assert!(!text.contains('\x1b'), "escapes stripped: {text:?}");
    assert!(text.contains("two") && text.contains("three"), "{text:?}");
}
