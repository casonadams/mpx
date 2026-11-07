//! Saving the session snapshot: when a save is due, what a snapshot records for each pane, and
//! which resume rule and session id a pane's process carries.

use std::time::Duration;

use futures_util::future::join_all;
use tokio::time::Instant;
use tracing::warn;

use super::Session;
use crate::config::ResumeRule;
use crate::daemon::snapshot::{PaneRecord, build_snapshot};
use crate::node::now_ms;
use crate::protocol::NodeInfo;
use crate::store::SessionSnapshot;

/// How long a change waits before the snapshot is written, so a burst of changes saves once.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

impl Session {
    pub(super) fn schedule_save(&mut self) {
        if !self.shutting_down && self.save_due.is_none() {
            self.save_due = Some(Instant::now() + SAVE_DEBOUNCE);
        }
    }

    /// Writes the snapshot. An unclean save is dropped once shutdown has begun.
    pub(super) async fn save(&mut self, clean: bool) {
        if self.shutting_down && !clean {
            return;
        }
        let snapshot = self.snapshot(clean).await;
        if let Err(e) = self.snapshots.save(&snapshot) {
            warn!("could not save session snapshot: {}", e);
        }
    }

    async fn snapshot(&self, clean: bool) -> SessionSnapshot {
        let panes: Vec<(String, &super::Entry)> = self
            .workspace
            .windows
            .iter()
            .flat_map(|window| window.leaves())
            .filter_map(|leaf| self.nodes.get(&leaf).map(|entry| (leaf, entry)))
            .collect();
        let infos = join_all(panes.iter().map(|(_, entry)| entry.handle.info(true))).await;
        let home = crate::config::home_dir();
        let records: Vec<PaneRecord> = panes
            .into_iter()
            .zip(infos)
            .map(|((leaf, entry), info)| {
                let info = info.ok();
                let cwd = info.as_ref().and_then(|i| i.cwd.clone());
                let fg = info.as_ref().and_then(|i| i.foreground.clone());
                let lookup_name = fg.as_deref().unwrap_or(&entry.opts.command);
                let (resume_rule, session_id) = resolve_resume(
                    lookup_name,
                    &self.config.resume,
                    info.as_ref(),
                    home.as_deref(),
                );
                PaneRecord {
                    name: leaf,
                    opts: entry.opts.clone(),
                    running: !entry.exited,
                    cwd,
                    resume_rule,
                    session_id,
                }
            })
            .collect();
        let mut snapshot =
            build_snapshot(&self.session, now_ms(), clean, &self.workspace, &records);
        snapshot.environment = self.stores.environment();
        snapshot
    }
}

/// The first resume rule that names the program, with the session id it resumes: read from the
/// program's own files, else from the rule's environment variable.
fn resolve_resume(
    lookup_name: &str,
    rules: &[ResumeRule],
    info: Option<&NodeInfo>,
    home: Option<&std::path::Path>,
) -> (Option<String>, Option<String>) {
    for rule in rules {
        let matches = rule.name.eq_ignore_ascii_case(lookup_name)
            || rule
                .detect
                .iter()
                .any(|d| d.eq_ignore_ascii_case(lookup_name));
        if matches {
            let from_files = rule.id_source.zip(home).and_then(|(source, home)| {
                let pid = info.and_then(|i| i.foreground_pid);
                let tty = info.and_then(|i| i.tty.as_deref());
                crate::store::lookup_session_id(source, home, pid, tty)
            });
            let session_id =
                from_files.or_else(|| rule.env_id.as_ref().and_then(|v| std::env::var(v).ok()));
            return (Some(rule.name.clone()), session_id);
        }
    }
    (None, None)
}
