//! Idempotency keys: the first request with a key runs, concurrent repeats wait for its response,
//! later repeats get the cached response. Pure state; the caller supplies the clock.
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

/// Keys remembered at most, finished and running alike.
pub const MAX_KEYS: usize = 1024;
/// How long a finished response is replayed.
pub const TTL: Duration = Duration::from_secs(10 * 60);

/// What a request with a key must do.
pub enum Claim<V> {
    /// First request for the key: run it, then `finish` or `abandon`.
    Run,
    /// The key is running: the receiver yields its response, or `None` when it was abandoned.
    Wait(oneshot::Receiver<Option<V>>),
    /// The key finished within the TTL.
    Cached(V),
    /// `MAX_KEYS` requests are running: the new key is refused until one of them ends.
    Busy,
}

enum Slot<V> {
    InFlight {
        waiters: Vec<oneshot::Sender<Option<V>>>,
    },
    Done(V),
}

pub struct IdemCache<V> {
    slots: HashMap<String, Slot<V>>,
    /// Finished keys, oldest first, with when each finished: the one `Done` slot of each.
    finished: VecDeque<(String, Instant)>,
}

impl<V> Default for IdemCache<V> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            finished: VecDeque::new(),
        }
    }
}

impl<V: Clone> IdemCache<V> {
    pub fn begin(&mut self, key: &str, now: Instant) -> Claim<V> {
        self.expire(now);
        match self.slots.get(key) {
            Some(Slot::Done(value)) => Claim::Cached(value.clone()),
            Some(Slot::InFlight { .. }) => {
                let (tx, rx) = oneshot::channel();
                if let Some(Slot::InFlight { waiters }) = self.slots.get_mut(key) {
                    waiters.push(tx);
                }
                Claim::Wait(rx)
            }
            None if self.in_flight() >= MAX_KEYS => Claim::Busy,
            None => {
                self.slots.insert(
                    key.to_string(),
                    Slot::InFlight {
                        waiters: Vec::new(),
                    },
                );
                Claim::Run
            }
        }
    }

    /// Records the response of the request that claimed `key` and answers its waiters.
    pub fn finish(&mut self, key: &str, value: V, now: Instant) {
        if let Some(Slot::InFlight { waiters }) = self.slots.remove(key) {
            for waiter in waiters {
                let _ = waiter.send(Some(value.clone()));
            }
        }
        self.slots.insert(key.to_string(), Slot::Done(value));
        self.finished.push_back((key.to_string(), now));
        self.evict();
    }

    /// The request that claimed `key` produced no response; waiters are told so and the key is freed.
    pub fn abandon(&mut self, key: &str) {
        if let Some(Slot::InFlight { waiters }) = self.slots.remove(key) {
            for waiter in waiters {
                let _ = waiter.send(None);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn expire(&mut self, now: Instant) {
        while let Some((key, at)) = self.finished.front() {
            if now.saturating_duration_since(*at) < TTL {
                break;
            }
            self.slots.remove(key);
            self.finished.pop_front();
        }
    }

    /// Running keys: the slots that are not finished.
    fn in_flight(&self) -> usize {
        self.slots.len() - self.finished.len()
    }

    /// Drops the oldest finished keys while more than `MAX_KEYS` have finished. Running keys do not
    /// count, so a finished response is not dropped for running requests.
    fn evict(&mut self) {
        while self.finished.len() > MAX_KEYS {
            let Some((key, _)) = self.finished.pop_front() else {
                break;
            };
            self.slots.remove(&key);
        }
    }
}
