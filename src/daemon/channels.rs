//! tmux `wait-for` channels: named rendezvous points with a pending-signal flag and a lock.
use std::collections::{HashMap, VecDeque};

use tokio::sync::oneshot;

#[derive(Default)]
struct Channel {
    /// A signal arrived while nobody waited; the next waiter returns at once.
    woken: bool,
    waiters: Vec<oneshot::Sender<()>>,
    locked: bool,
    lockers: VecDeque<oneshot::Sender<()>>,
}

impl Channel {
    fn idle(&self) -> bool {
        !self.woken && self.waiters.is_empty() && !self.locked && self.lockers.is_empty()
    }
}

#[derive(Default)]
pub(super) struct Channels(HashMap<String, Channel>);

impl Channels {
    /// `None` when a signal was already pending (consumed); else a receiver that fires on the next
    /// signal.
    pub(super) fn wait(&mut self, name: &str) -> Option<oneshot::Receiver<()>> {
        let channel = self.0.entry(name.to_string()).or_default();
        let pending = if channel.woken {
            channel.woken = false;
            None
        } else {
            let (tx, rx) = oneshot::channel();
            channel.waiters.push(tx);
            Some(rx)
        };
        self.sweep(name);
        pending
    }

    /// Wakes every waiter, or leaves one pending signal when there are none.
    pub(super) fn signal(&mut self, name: &str) {
        let channel = self.0.entry(name.to_string()).or_default();
        if channel.waiters.is_empty() {
            channel.woken = true;
        } else {
            for waiter in channel.waiters.drain(..) {
                let _ = waiter.send(());
            }
        }
        self.sweep(name);
    }

    /// `None` when the lock was free and is now held; else a receiver that fires when it is handed
    /// over.
    pub(super) fn lock(&mut self, name: &str) -> Option<oneshot::Receiver<()>> {
        let channel = self.0.entry(name.to_string()).or_default();
        if channel.locked {
            let (tx, rx) = oneshot::channel();
            channel.lockers.push_back(tx);
            return Some(rx);
        }
        channel.locked = true;
        None
    }

    /// Hands the lock to the longest-waiting locker, or frees it.
    pub(super) fn unlock(&mut self, name: &str) -> Result<(), String> {
        let channel = self
            .0
            .get_mut(name)
            .filter(|c| c.locked)
            .ok_or_else(|| format!("channel {name} not locked"))?;
        channel.locked = false;
        while let Some(next) = channel.lockers.pop_front() {
            if next.send(()).is_ok() {
                channel.locked = true;
                break;
            }
        }
        self.sweep(name);
        Ok(())
    }

    fn sweep(&mut self, name: &str) {
        if self.0.get(name).is_some_and(Channel::idle) {
            self.0.remove(name);
        }
    }
}
