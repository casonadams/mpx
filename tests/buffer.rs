use mpx::buffer::{RingBuffer, strip_ansi};

#[test]
fn writes_append_until_capacity() {
    let mut rb = RingBuffer::new(10);
    rb.write(b"hello");
    assert_eq!(rb.snapshot(), b"hello");
    rb.write(b"world");
    assert_eq!(rb.snapshot(), b"helloworld");
    rb.write(b"123");
    assert_eq!(rb.snapshot(), b"loworld123");
}

#[test]
fn oversized_write_keeps_newest_bytes() {
    let mut rb = RingBuffer::new(5);
    rb.write(b"0123456789");
    assert_eq!(rb.snapshot(), b"56789");
    assert_eq!(rb.total_written(), 10);
}

#[test]
fn tail_lines_returns_last_n_lines() {
    let mut rb = RingBuffer::new(100);
    rb.write(b"line 1\nline 2\nline 3\nline 4\n");
    assert_eq!(rb.tail_lines(2), "line 3\nline 4\n");
}

#[test]
fn tail_lines_handles_edges() {
    let mut rb = RingBuffer::new(100);
    assert_eq!(rb.tail_lines(0), "");
    assert_eq!(rb.tail_lines(5), "");

    rb.write(b"single line without newline");
    assert_eq!(rb.tail_lines(1), "single line without newline\n");
    assert_eq!(rb.tail_lines(5), "single line without newline\n");

    let mut rb2 = RingBuffer::new(100);
    rb2.write(b"a\nb\nc\nd\ne");
    assert_eq!(rb2.tail_lines(2), "d\ne\n");
    assert_eq!(rb2.tail_lines(3), "c\nd\ne\n");
    assert_eq!(rb2.tail_lines(10), "a\nb\nc\nd\ne\n");
}

#[test]
fn since_returns_only_bytes_after_offset() {
    let mut rb = RingBuffer::new(100);
    rb.write(b"abc");
    let (first, next) = rb.since(0);
    assert_eq!(first, b"abc");
    assert_eq!(next, 3);

    rb.write(b"def");
    let (delta, next) = rb.since(next);
    assert_eq!(delta, b"def");
    assert_eq!(next, 6);
}

#[test]
fn since_older_than_retained_starts_at_oldest_byte() {
    let mut rb = RingBuffer::new(4);
    rb.write(b"0123456789");
    let (bytes, next) = rb.since(2);
    assert_eq!(bytes, b"6789");
    assert_eq!(next, 10);
}

#[test]
fn since_future_offset_is_empty() {
    let mut rb = RingBuffer::new(16);
    rb.write(b"xy");
    let (bytes, next) = rb.since(99);
    assert!(bytes.is_empty());
    assert_eq!(next, 2);
}

#[test]
fn strip_ansi_removes_csi_and_osc_sequences() {
    let raw = b"\x1b[31mred\x1b[0m \x1b]0;title\x07ok\x1b]2;t\x1b\\!";
    assert_eq!(strip_ansi(raw), "red ok!");
}

#[test]
fn strip_ansi_drops_carriage_returns_and_bells_keeps_newlines() {
    assert_eq!(strip_ansi(b"a\r\nb\x07\tc"), "a\nb\tc");
}

#[test]
fn strip_ansi_handles_charset_designation_and_truncated_escape() {
    assert_eq!(strip_ansi(b"\x1b(Bplain"), "plain");
    assert_eq!(strip_ansi(b"tail\x1b["), "tail");
}
