//! Saving and restoring pane scrollback while `restore` is on: each running terminal pane's newest
//! output is written next to the snapshot, so a restore after a crash or reboot shows it again.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use futures_util::future::join_all;
use tokio::sync::oneshot;
use tracing::warn;

use super::Session;
use crate::node::pane::PaneHandle;
use crate::store::{PaneScrollback, ScrollbackStore, SessionSnapshot, SnapshotStore};
use crate::vt::capture::restored_scrollback;

enum Job {
    Snapshot(Box<SessionSnapshot>),
    Save(Vec<(String, PaneScrollback)>),
    /// Deletes the scrollback of every pane but these.
    Retain(Vec<String>),
    /// Answers once every earlier job is done.
    Sync(oneshot::Sender<()>),
}

/// Writes the snapshot and scrollback files on its own thread, in the order the jobs were sent, so
/// neither the session nor its panes wait for the disk. The thread ends with the writer.
pub(super) struct ScrollbackWriter {
    store: ScrollbackStore,
    jobs: mpsc::Sender<Job>,
}

impl ScrollbackWriter {
    pub(super) fn spawn(store: ScrollbackStore, snapshots: SnapshotStore) -> Self {
        let (jobs, rx) = mpsc::channel();
        let disk = store.clone();
        thread::spawn(move || {
            for job in rx {
                run(&snapshots, &disk, job);
            }
        });
        Self { store, jobs }
    }

    fn send(&self, job: Job) {
        if self.jobs.send(job).is_err() {
            warn!("scrollback writer has stopped");
        }
    }

    /// Queues `snapshot` to be written.
    pub(super) fn save_snapshot(&self, snapshot: SessionSnapshot) {
        self.send(Job::Snapshot(Box::new(snapshot)));
    }

    /// Waits until every job sent so far is written.
    pub(super) async fn sync(&self) {
        let (done, wait) = oneshot::channel();
        self.send(Job::Sync(done));
        let _ = wait.await;
    }
}

fn run(snapshots: &SnapshotStore, store: &ScrollbackStore, job: Job) {
    match job {
        Job::Snapshot(snapshot) => {
            if let Err(e) = snapshots.save(&snapshot) {
                warn!("could not save session snapshot: {}", e);
            }
        }
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
    /// Hands the output each running terminal pane wrote since its last save to the writer, once
    /// enough of it is due or `force` says to.
    pub(super) async fn flush_scrollback(&self, force: bool) {
        if !self.config.session.restore || self.shutting_down {
            return;
        }
        let handles: Vec<PaneHandle> = self
            .nodes
            .values()
            .filter(|entry| !entry.exited && entry.handle.pty())
            .map(|entry| entry.handle.clone())
            .collect();
        let unsaved = join_all(
            handles
                .iter()
                .map(|handle| handle.unsaved_scrollback(force)),
        )
        .await;
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

    /// What each of the restored panes `names` shows before its process writes: its saved
    /// scrollback, when `restore` is on and there is one. Replayed on the blocking pool, panes in
    /// parallel, so the session keeps serving while a large history is read.
    pub(super) async fn restored_scrollbacks(
        &self,
        names: Vec<String>,
    ) -> HashMap<String, Vec<u8>> {
        if !self.config.session.restore {
            return HashMap::new();
        }
        let lines = self.config.session.scrollback_lines;
        let loads = names.into_iter().map(|pane| {
            let store = self.scrollback.store.clone();
            tokio::task::spawn_blocking(move || {
                let restored = load_restored(&store, &pane, lines);
                (pane, restored)
            })
        });
        join_all(loads)
            .await
            .into_iter()
            .filter_map(|joined| joined.ok())
            .filter(|(_, restored)| !restored.is_empty())
            .collect()
    }
}

fn load_restored(store: &ScrollbackStore, pane: &str, lines: usize) -> Vec<u8> {
    match store.load(pane) {
        Ok(Some(saved)) => {
            let size = (usize::from(saved.size.0), usize::from(saved.size.1));
            restored_scrollback(&saved.log, size, lines)
        }
        Ok(None) => Vec::new(),
        Err(e) => {
            warn!("could not load scrollback of pane '{pane}': {e}");
            Vec::new()
        }
    }
}
