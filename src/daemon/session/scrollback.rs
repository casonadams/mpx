//! Saving and restoring pane scrollback while `restore` is on: each running terminal pane's newest
//! output is written next to the snapshot, so a restore after a crash or reboot shows it again.

use std::sync::mpsc;
use std::thread;

use futures_util::future::join_all;
use tokio::sync::oneshot;
use tracing::warn;

use super::Session;
use crate::node::pane::PaneHandle;
use crate::store::{PaneScrollback, ScrollbackStore};
use crate::vt::capture::restored_scrollback;

enum Job {
    Save(Vec<(String, PaneScrollback)>),
    /// Deletes the scrollback of every pane but these.
    Retain(Vec<String>),
    /// Answers once every earlier job is done.
    Sync(oneshot::Sender<()>),
}

/// Writes scrollback files on its own thread, in the order the jobs were sent, so neither the
/// session nor its panes wait for the disk. The thread ends with the writer.
pub(super) struct ScrollbackWriter {
    store: ScrollbackStore,
    jobs: mpsc::Sender<Job>,
}

impl ScrollbackWriter {
    pub(super) fn spawn(store: ScrollbackStore) -> Self {
        let (jobs, rx) = mpsc::channel();
        let disk = store.clone();
        thread::spawn(move || {
            for job in rx {
                run(&disk, job);
            }
        });
        Self { store, jobs }
    }

    fn send(&self, job: Job) {
        if self.jobs.send(job).is_err() {
            warn!("scrollback writer has stopped");
        }
    }

    /// Waits until every job sent so far is written.
    pub(super) async fn sync(&self) {
        let (done, wait) = oneshot::channel();
        self.send(Job::Sync(done));
        let _ = wait.await;
    }
}

fn run(store: &ScrollbackStore, job: Job) {
    match job {
        Job::Save(panes) => {
            for (pane, scrollback) in panes {
                if let Err(e) = store.save(&pane, &scrollback) {
                    warn!("could not save scrollback of pane '{pane}': {e}");
                }
            }
        }
        Job::Retain(panes) => {
            if let Err(e) = store.retain(&panes) {
                warn!("could not remove stale scrollback: {e}");
            }
        }
        Job::Sync(done) => {
            let _ = done.send(());
        }
    }
}

impl Session {
    /// Hands the output each running terminal pane wrote since its last save to the writer.
    pub(super) async fn flush_scrollback(&self) {
        if !self.config.session.restore || self.shutting_down {
            return;
        }
        let handles: Vec<PaneHandle> = self
            .nodes
            .values()
            .filter(|entry| !entry.exited && entry.handle.pty())
            .map(|entry| entry.handle.clone())
            .collect();
        let unsaved = join_all(handles.iter().map(PaneHandle::unsaved_scrollback)).await;
        let saves: Vec<(String, PaneScrollback)> = handles
            .iter()
            .zip(unsaved)
            .filter_map(|(handle, unsaved)| {
                let (log, size) = unsaved.ok()??;
                Some((handle.name().to_string(), PaneScrollback { size, log }))
            })
            .collect();
        if !saves.is_empty() {
            self.scrollback.send(Job::Save(saves));
        }
    }

    /// Keeps the saved scrollback of the snapshot's `panes` only, or of none while `restore` is
    /// off.
    pub(super) fn retain_scrollback(&self, mut panes: Vec<String>) {
        if !self.config.session.restore {
            panes.clear();
        }
        self.scrollback.send(Job::Retain(panes));
    }

    /// What restored pane `pane` shows before its process writes: its saved scrollback, when
    /// `restore` is on and there is one.
    pub(super) fn restored_scrollback(&self, pane: &str) -> Vec<u8> {
        if !self.config.session.restore {
            return Vec::new();
        }
        match self.scrollback.store.load(pane) {
            Ok(Some(saved)) => {
                let size = (usize::from(saved.size.0), usize::from(saved.size.1));
                restored_scrollback(&saved.log, size, self.config.session.scrollback_lines)
            }
            Ok(None) => Vec::new(),
            Err(e) => {
                warn!("could not load scrollback of pane '{pane}': {e}");
                Vec::new()
            }
        }
    }
}
