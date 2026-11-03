//! `wait-for` channels. A signal wakes the channel's waiters; with none waiting, the next wait
//! returns at once. A lock is held by one taker, and the others queue behind it in order.
use std::collections::{HashMap, VecDeque};

use tokio::sync::oneshot;

/// Answers one blocked `wait-for`: `Ok` once it may go on.
pub type Reply = oneshot::Sender<Result<(), String>>;

#[derive(Default)]
pub struct Channels {
    channels: HashMap<String, Channel>,
}

#[derive(Default)]
struct Channel {
    /// A signal came with no waiter; the next wait consumes it.
    signalled: bool,
    waiters: Vec<Reply>,
    locked: bool,
    takers: VecDeque<Reply>,
}

impl Channel {
    fn idle(&self) -> bool {
        !self.signalled && self.waiters.is_empty() && !self.locked && self.takers.is_empty()
    }
}

fn answer(reply: Reply) {
    let _ = reply.send(Ok(()));
}

impl Channels {
    /// `wait-for channel`: answers `reply` at once if a signal is pending, else when one comes.
    pub fn wait(&mut self, channel: &str, reply: Reply) {
        let entry = self.entry(channel);
        if entry.signalled {
            entry.signalled = false;
            answer(reply);
        } else {
            entry.waiters.push(reply);
        }
        self.tidy(channel);
    }

    /// `wait-for -S channel`: answers every waiter, or keeps one signal for the next wait.
    pub fn signal(&mut self, channel: &str) {
        let entry = self.entry(channel);
        if entry.waiters.is_empty() {
            entry.signalled = true;
        } else {
            entry.waiters.drain(..).for_each(answer);
        }
        self.tidy(channel);
    }

    /// `wait-for -L channel`: answers `reply` at once if the lock is free, else when it is handed
    /// over.
    pub fn lock_channel(&mut self, channel: &str, reply: Reply) {
        let entry = self.entry(channel);
        if entry.locked {
            entry.takers.push_back(reply);
        } else {
            entry.locked = true;
            answer(reply);
        }
        self.tidy(channel);
    }

    /// `wait-for -U channel`: hands the lock to the next taker still waiting, or frees it. A
    /// channel that is not locked is an error.
    pub fn unlock(&mut self, channel: &str) -> Result<(), String> {
        let entry = self
            .channels
            .get_mut(channel)
            .filter(|c| c.locked)
            .ok_or_else(|| format!("channel {channel} is not locked"))?;
        let next = loop {
            match entry.takers.pop_front() {
                Some(taker) if taker.is_closed() => continue,
                next => break next,
            }
        };
        match next {
            Some(taker) => answer(taker),
            None => entry.locked = false,
        }
        self.tidy(channel);
        Ok(())
    }

    fn entry(&mut self, channel: &str) -> &mut Channel {
        self.channels.entry(channel.to_string()).or_default()
    }

    fn tidy(&mut self, channel: &str) {
        if self.channels.get(channel).is_some_and(Channel::idle) {
            self.channels.remove(channel);
        }
    }
}
