//! Idempotency keys: the first request with a key runs, concurrent repeats wait for its response,
//! later repeats get the cached response. Pure state; the caller supplies the clock.
use std::collections::HashMap;

use tokio::sync::oneshot;

/// Keys remembered at most.
pub const MAX_KEYS: usize = 1024;
/// How long a finished response is replayed.
pub const TTL_MS: u64 = 10 * 60 * 1000;

/// What a request with a key must do.
pub enum Claim<V> {
    /// First request for the key: run it, then `finish` or `abandon`.
    Run,
    /// The key is running: the receiver yields its response, or `None` when it was abandoned.
    Wait(oneshot::Receiver<Option<V>>),
    /// The key finished within the TTL.
    Cached(V),
}

enum Slot<V> {
    InFlight {
        waiters: Vec<oneshot::Sender<Option<V>>>,
    },
    Done {
        value: V,
        at_ms: u64,
        seq: u64,
    },
}

pub struct IdemCache<V> {
    slots: HashMap<String, Slot<V>>,
    seq: u64,
}

impl<V> Default for IdemCache<V> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            seq: 0,
        }
    }
}

impl<V: Clone> IdemCache<V> {
    pub fn begin(&mut self, key: &str, now_ms: u64) -> Claim<V> {
        self.expire(now_ms);
        match self.slots.get(key) {
            Some(Slot::Done { value, .. }) => Claim::Cached(value.clone()),
            Some(Slot::InFlight { .. }) => {
                let (tx, rx) = oneshot::channel();
                if let Some(Slot::InFlight { waiters }) = self.slots.get_mut(key) {
                    waiters.push(tx);
                }
                Claim::Wait(rx)
            }
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
    pub fn finish(&mut self, key: &str, value: V, now_ms: u64) {
        if let Some(Slot::InFlight { waiters }) = self.slots.remove(key) {
            for waiter in waiters {
                let _ = waiter.send(Some(value.clone()));
            }
        }
        self.seq += 1;
        self.slots.insert(
            key.to_string(),
            Slot::Done {
                value,
                at_ms: now_ms,
                seq: self.seq,
            },
        );
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

    fn expire(&mut self, now_ms: u64) {
        self.slots.retain(|_, slot| match slot {
            Slot::Done { at_ms, .. } => now_ms.saturating_sub(*at_ms) < TTL_MS,
            Slot::InFlight { .. } => true,
        });
    }

    /// Drops the oldest finished keys while over the bound. Running keys are never dropped.
    fn evict(&mut self) {
        while self.slots.len() > MAX_KEYS {
            let oldest = self
                .slots
                .iter()
                .filter_map(|(key, slot)| match slot {
                    Slot::Done { seq, .. } => Some((*seq, key.clone())),
                    Slot::InFlight { .. } => None,
                })
                .min();
            match oldest {
                Some((_, key)) => {
                    self.slots.remove(&key);
                }
                None => break,
            }
        }
    }
}
