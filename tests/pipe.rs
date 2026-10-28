use mpx::pipe::VirtualPipe;

fn pipe(capacity: usize) -> VirtualPipe {
    VirtualPipe::new("bus".to_string(), capacity)
}

#[test]
fn sequences_start_at_one_and_increase() {
    let mut p = pipe(1024);
    assert_eq!(p.write("first", 1), 1);
    assert_eq!(p.write("second", 2), 2);
    assert_eq!(p.last_seq(), 2);
    assert_eq!(p.info().next_seq, 3);
    assert_eq!(p.info().messages, 2);
}

#[test]
fn read_since_is_exclusive_and_honours_limit() {
    let mut p = pipe(1024);
    for (i, text) in ["a", "b", "c"].iter().enumerate() {
        p.write(text, i as u64);
    }
    let after_first: Vec<u64> = p.read(1, None).iter().map(|m| m.seq).collect();
    assert_eq!(after_first, vec![2, 3]);

    let limited = p.read(0, Some(1));
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].data, "a");
}

#[test]
fn capacity_drops_oldest_messages() {
    let mut p = pipe(10);
    p.write("aaaa", 0);
    p.write("bbbb", 0);
    p.write("cccc", 0);
    let kept = p.read(0, None);
    let data: Vec<&str> = kept.iter().map(|m| m.data.as_str()).collect();
    assert_eq!(data, vec!["bbbb", "cccc"]);
    assert_eq!(kept[0].seq, 2);
}

#[test]
fn oversized_single_message_is_retained() {
    let mut p = pipe(4);
    let seq = p.write("much longer than capacity", 0);
    let kept = p.read(0, None);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].seq, seq);
}

#[test]
fn consumer_cursor_advances_on_each_read() {
    let mut p = pipe(1024);
    p.write("one", 0);
    p.write("two", 0);

    let first = p.read_consumer("reader", 0, None);
    assert_eq!(first.len(), 2);
    assert_eq!(p.cursor("reader"), Some(2));

    assert!(p.read_consumer("reader", 0, None).is_empty());
    p.write("three", 0);
    let next = p.read_consumer("reader", 0, None);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].data, "three");

    assert_eq!(p.cursor("other"), None);
    assert_eq!(p.read_consumer("other", 0, None).len(), 3);
}

#[test]
fn new_consumer_starts_after_since_and_keeps_its_cursor() {
    let mut p = VirtualPipe::new("bus".into(), 1024);
    for m in ["one", "two", "three"] {
        p.write(m, 0);
    }
    let first = p.read_consumer("late", 2, None);
    assert_eq!(
        first.iter().map(|m| m.data.as_str()).collect::<Vec<_>>(),
        ["three"]
    );
    assert!(p.read_consumer("late", 0, None).is_empty());
    assert!(p.read_consumer("empty", 3, None).is_empty());
    assert_eq!(p.cursor("empty"), Some(3));
}

#[test]
fn empty_pipe_reports_zero_last_sequence() {
    let p = pipe(16);
    assert_eq!(p.last_seq(), 0);
    assert!(p.read(0, None).is_empty());
}
