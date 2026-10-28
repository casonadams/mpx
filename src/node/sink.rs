//! Bounded, non-blocking delivery of stream events to one subscriber.
use tokio::sync::mpsc::{self, error::TrySendError};

use crate::protocol::StreamEvent;

/// Events a subscriber may hold before overflow starts dropping.
pub const SUB_QUEUE: usize = 4096;

/// What to do with one event offered to a subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Deliver,
    /// Deliver `Lagged` in place of the event (once until the queue drains).
    Lagged,
    Drop,
}

/// Overflow policy of one subscriber. `free` is the queue's remaining slots and `capacity` its size.
/// One slot is reserved for `Lagged`, so a full queue still tells the reader that events were lost.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Feed {
    lagged: bool,
}

impl Feed {
    pub fn step(&mut self, free: usize, capacity: usize) -> Step {
        if free == capacity {
            self.lagged = false;
        }
        if free > 1 {
            Step::Deliver
        } else if !self.lagged {
            self.lagged = true;
            Step::Lagged
        } else {
            Step::Drop
        }
    }
}

/// Sending half of a subscription. Never blocks: a slow reader loses events instead.
pub struct Sink {
    tx: mpsc::Sender<StreamEvent>,
    feed: Feed,
}

impl Sink {
    /// A sink and the receiver the subscriber reads from.
    pub fn channel() -> (Sink, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(SUB_QUEUE + 1);
        (
            Sink {
                tx,
                feed: Feed::default(),
            },
            rx,
        )
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Offers an event. Returns false once the subscriber is gone.
    pub fn push(&mut self, event: StreamEvent) -> bool {
        if self.tx.is_closed() {
            return false;
        }
        let free = self.tx.capacity();
        match self.feed.step(free, self.tx.max_capacity()) {
            Step::Deliver => self.send(event),
            Step::Lagged => self.send(StreamEvent::Lagged),
            Step::Drop => true,
        }
    }

    fn send(&self, event: StreamEvent) -> bool {
        match self.tx.try_send(event) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Closed(_)) => false,
        }
    }
}
