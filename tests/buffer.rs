use mpx::buffer::{RingBuffer, Utf8Stream, strip_ansi};

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
fn oldest_offset_tracks_the_retained_window() {
    let mut rb = RingBuffer::new(4);
    rb.write(b"abcdef");
    assert_eq!(rb.total_written(), 6);
    assert_eq!(rb.oldest_offset(), 2);
    assert_eq!(rb.since(0).0, b"cdef");
}

#[test]
fn utf8_stream_holds_a_split_sequence_until_it_completes() {
    let mut stream = Utf8Stream::default();
    let euro = "€".as_bytes();
    let (text, held) = stream.feed(&euro[..2]);
    assert_eq!((text.as_str(), held), ("", 2));
    let (text, held) = stream.feed(&euro[2..]);
    assert_eq!((text.as_str(), held), ("€", 0));
}

#[test]
fn utf8_stream_replaces_invalid_bytes_and_keeps_going() {
    let mut stream = Utf8Stream::default();
    let (text, held) = stream.feed(b"a\xffb");
    assert_eq!((text.as_str(), held), ("a\u{FFFD}b", 0));
}

#[test]
fn utf8_stream_decodes_a_four_byte_char_fed_one_byte_at_a_time() {
    let mut stream = Utf8Stream::default();
    let bytes = "😀".as_bytes();
    for (i, byte) in bytes[..3].iter().enumerate() {
        assert_eq!(
            stream.feed(std::slice::from_ref(byte)),
            (String::new(), i + 1)
        );
    }
    assert_eq!(stream.feed(&bytes[3..]), ("😀".to_string(), 0));
}

#[test]
fn utf8_stream_holds_at_most_a_sequence_prefix() {
    let mut stream = Utf8Stream::default();
    let (text, held) = stream.feed(b"ok\xe2\x82");
    assert_eq!((text.as_str(), held), ("ok", 2));
    let (text, held) = stream.feed(b"x");
    assert_eq!((text.as_str(), held), ("\u{FFFD}x", 0));
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
fn since_wrapped_buffer_slices() {
    let mut rb = RingBuffer::new(6);
    rb.write(b"abcdef");
    // Writing 3 more evicts 3, leaving "defghi" wrapped across slices in VecDeque.
    rb.write(b"ghi");
    assert_eq!(rb.oldest_offset(), 3);
    assert_eq!(rb.total_written(), 9);

    // Read spanning both slices:
    let (all, _) = rb.since(3);
    assert_eq!(all, b"defghi");

    // Read starting partially in first slice:
    let (part_first, _) = rb.since(4);
    assert_eq!(part_first, b"efghi");

    // Read starting in second slice:
    let (second_slice, _) = rb.since(7);
    assert_eq!(second_slice, b"hi");
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
