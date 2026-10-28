use std::collections::{HashMap, VecDeque};
use tokio::sync::watch;

use crate::protocol::{PipeInfo, PipeMessage};

/// Named in-memory message queue shared by agents. Messages are numbered from 1 and the oldest
/// are dropped once the retained bytes exceed the capacity.
#[derive(Debug)]
pub struct VirtualPipe {
    name: String,
    capacity: usize,
    messages: VecDeque<PipeMessage>,
    bytes: usize,
    next_seq: u64,
    cursors: HashMap<String, u64>,
    tip: watch::Sender<u64>,
}

impl VirtualPipe {
    pub fn new(name: String, capacity: usize) -> Self {
        let (tip, _) = watch::channel(0);
        Self {
            name,
            capacity,
            messages: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
            cursors: HashMap::new(),
            tip,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Appends a message and returns its sequence number.
    pub fn write(&mut self, data: &str, ts_ms: u64) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.bytes += data.len();
        self.messages.push_back(PipeMessage {
            seq,
            ts_ms,
            data: data.to_string(),
        });
        while self.bytes > self.capacity && self.messages.len() > 1 {
            if let Some(old) = self.messages.pop_front() {
                self.bytes -= old.data.len();
            }
        }
        self.tip.send_replace(seq);
        seq
    }

    /// Messages with `seq > after`, oldest first.
    pub fn read(&self, after: u64, limit: Option<usize>) -> Vec<PipeMessage> {
        self.messages
            .iter()
            .filter(|m| m.seq > after)
            .take(limit.unwrap_or(usize::MAX))
            .cloned()
            .collect()
    }

    /// Messages after the consumer's cursor; a consumer without a cursor starts after `start`.
    /// The cursor is registered and advances past the last message returned.
    pub fn read_consumer(&mut self, consumer: &str, start: u64, limit: Option<usize>) -> Vec<PipeMessage> {
        let cursor = self.cursor(consumer).unwrap_or(start);
        let out = self.read(cursor, limit);
        let advanced = out.last().map_or(cursor, |m| m.seq);
        self.cursors.insert(consumer.to_string(), advanced);
        out
    }

    /// Sequence number the consumer has read up to; `None` before its first read.
    pub fn cursor(&self, consumer: &str) -> Option<u64> {
        self.cursors.get(consumer).copied()
    }

    /// Sequence number of the newest message (0 when empty).
    pub fn last_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Receiver that observes the newest sequence number after every write.
    pub fn watch_tip(&self) -> watch::Receiver<u64> {
        self.tip.subscribe()
    }

    pub fn info(&self) -> PipeInfo {
        PipeInfo {
            name: self.name.clone(),
            messages: self.messages.len(),
            next_seq: self.next_seq,
        }
    }
}
