use mpx::node::sink::{Feed, SUB_QUEUE, Sink, Step};
use mpx::protocol::StreamEvent;

fn output(offset: u64) -> StreamEvent {
    StreamEvent::Output {
        name: "p".to_string(),
        data: "x".to_string(),
        offset,
        next: offset + 1,
    }
}

#[test]
fn full_queue_delivers_then_one_lagged_then_drops() {
    let mut feed = Feed::default();
    let capacity = SUB_QUEUE + 1;
    let mut free = capacity;
    let mut delivered = 0;
    while free > 1 {
        assert_eq!(feed.step(free, capacity), Step::Deliver);
        free -= 1;
        delivered += 1;
    }
    assert_eq!(delivered, SUB_QUEUE);
    assert_eq!(feed.step(free, capacity), Step::Lagged);
    free -= 1;
    assert_eq!(feed.step(free, capacity), Step::Drop);
    assert_eq!(feed.step(free, capacity), Step::Drop);
}

#[test]
fn draining_the_queue_rearms_lagged() {
    let mut feed = Feed::default();
    assert_eq!(feed.step(2, 4), Step::Deliver);
    assert_eq!(feed.step(1, 4), Step::Lagged);
    assert_eq!(feed.step(0, 4), Step::Drop);
    assert_eq!(feed.step(4, 4), Step::Deliver);
    assert_eq!(feed.step(1, 4), Step::Lagged);
}

#[test]
fn overflowing_sink_yields_one_lagged_and_keeps_accepting() {
    let (mut sink, mut rx) = Sink::channel(None);
    for offset in 0..(SUB_QUEUE as u64 + 10) {
        assert!(
            sink.push(output(offset)),
            "a slow reader never stops the producer"
        );
    }
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    let lagged = events
        .iter()
        .filter(|e| matches!(e, StreamEvent::Lagged))
        .count();
    assert_eq!(lagged, 1);
    assert_eq!(events.len(), SUB_QUEUE + 1);

    assert!(sink.push(output(9_999)));
    assert!(matches!(
        rx.try_recv(),
        Ok(StreamEvent::Output { offset: 9_999, .. })
    ));
}
