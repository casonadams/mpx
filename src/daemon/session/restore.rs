//! Restoring a saved snapshot: the panes are spawned again with their saved scrollback, their
//! windows rebuilt from the part of each layout that survived, and linked windows shown at their
//! restored indices.

use std::collections::HashMap;

use tracing::warn;

use super::Session;
use crate::layout::RestoredLayout;
use crate::layout::geometry::Rect;
use crate::node::now_ms;
use crate::node::pane::TermView;
use crate::protocol::{Response, SpawnOpts};
use crate::store::{SessionSnapshot, WindowPlan, plan_restore};

impl Session {
    /// `restore`: restores the saved snapshot into a session with no panes.
    pub(super) async fn restore(&mut self) -> Response {
        if !self.nodes.is_empty() {
            return Response::Error("Session already has panes".to_string());
        }
        match self.snapshots.load() {
            Ok(Some(snapshot)) => {
                self.restore_snapshot(&snapshot).await;
                Response::Ok
            }
            Ok(None) => Response::Error("No snapshot to restore".to_string()),
            Err(e) => {
                self.keep_unloadable_snapshot(&e);
                Response::Error(e)
            }
        }
    }

    /// Restores the snapshot at startup, when the config asks for it.
    pub(super) async fn startup_restore(&mut self) {
        if !self.config.session.restore {
            return;
        }
        match self.snapshots.load() {
            Ok(Some(snapshot)) => self.restore_snapshot(&snapshot).await,
            Ok(None) => {}
            Err(e) => self.keep_unloadable_snapshot(&e),
        }
    }

    /// Moves a snapshot that cannot be loaded aside, so the next save does not overwrite it, and
    /// keeps the saved scrollback, which the lost snapshot named.
    fn keep_unloadable_snapshot(&mut self, error: &str) {
        warn!("could not load session snapshot: {error}");
        self.keep_scrollback = true;
        match self.snapshots.quarantine(now_ms()) {
            Ok(Some(path)) => warn!("moved the unreadable snapshot to {}", path.display()),
            Ok(None) => {}
            Err(e) => warn!("{e}"),
        }
    }

    async fn restore_snapshot(&mut self, snapshot: &SessionSnapshot) {
        self.stores.restore_environment(&snapshot.environment);
        let plan = plan_restore(snapshot, &self.config.resume);
        let names = plan
            .windows
            .iter()
            .flat_map(|window| window.panes.iter().map(|opts| opts.name.clone()))
            .collect();
        let mut scrollbacks = self.restored_scrollbacks(names).await;
        // What each restored index shows, by its position among the restored windows.
        let mut shown: Vec<Option<(usize, usize)>> = Vec::with_capacity(plan.windows.len());
        for window in plan.windows {
            let restored = match window.shares {
                Some(owner) => self.restore_link(owner, &shown, window.number),
                None => self.restore_window(window, &mut scrollbacks),
            };
            shown.push(restored);
        }
        if !self.workspace.links.is_empty() {
            let number_of =
                |survivor: usize| shown.get(survivor).copied().flatten().map(|(n, _)| n);
            match number_of(plan.active_window) {
                Some(number) => {
                    if !self.workspace.select_window(number) {
                        warn!("could not select restored window {number}");
                    }
                }
                None => warn!("could not select restored window {}", plan.active_window),
            }
            self.workspace
                .set_last_window(plan.last_window.and_then(number_of));
        }
        self.restored_ms = Some(now_ms());
        self.schedule_save();
    }

    /// Spawns the window's panes, then installs it with the surviving part of its layout. Returns the
    /// installed window's index and window id, or None when no pane of it survived.
    fn restore_window(
        &mut self,
        window: WindowPlan,
        scrollbacks: &mut HashMap<String, Vec<u8>>,
    ) -> Option<(usize, usize)> {
        let mut root = Some(window.layout);
        let (width, height) = self.window_size(self.requested);
        let planned = root
            .as_ref()
            .map(|r| r.compute_rects(Rect::new(0, 0, width, height)))
            .unwrap_or_default();
        for opts in &window.panes {
            let size = planned
                .iter()
                .find(|(name, _)| *name == opts.name)
                .map_or((width, height), |(_, rect)| (rect.width, rect.height));
            let scrollback = scrollbacks.remove(&opts.name).unwrap_or_default();
            match self.spawn_restored(opts, scrollback, size) {
                Ok(()) => {
                    if let Some(line) = window.startup_input.get(&opts.name) {
                        self.type_into(&opts.name, line);
                    }
                }
                Err(e) => {
                    warn!("could not restore pane '{}': {}", opts.name, e);
                    root = root.and_then(|r| r.remove_leaf(&opts.name));
                }
            }
        }
        let root = root?;
        let label = root.all_leaves().into_iter().next().unwrap_or_default();
        let number = self.workspace.install_window(
            window.number,
            label,
            window.name,
            RestoredLayout {
                root,
                focused: window.focused,
            },
        );
        let id = self.workspace.window_id_at(number)?;
        Some((number, id))
    }

    /// Shows the window that restored index `owner` at index `number` as well. None when the owner
    /// was not restored.
    fn restore_link(
        &mut self,
        owner: usize,
        shown: &[Option<(usize, usize)>],
        number: usize,
    ) -> Option<(usize, usize)> {
        let (_, id) = shown.get(owner).copied().flatten()?;
        Some((self.workspace.link_restored(id, number), id))
    }

    /// Types `line` into pane `name`; the terminal queues it until the shell reads it.
    fn type_into(&self, name: &str, line: &str) {
        let Some(entry) = self.nodes.get(name) else {
            return;
        };
        let handle = entry.handle.clone();
        let bytes = line.as_bytes().to_vec();
        tokio::spawn(async move {
            let _ = handle.input(Box::new(move |_: &TermView| bytes)).await;
        });
    }

    /// Spawns a restored pane at `size`, the size it has in its planned layout.
    fn spawn_restored(
        &mut self,
        opts: &SpawnOpts,
        restored: Vec<u8>,
        size: (u16, u16),
    ) -> Result<(), String> {
        self.check_name(&opts.name)?;
        let id = self.next_pane_id;
        let handle = self.spawn_pane(&opts.name, id, opts, opts.cwd.clone(), restored, size)?;
        self.next_pane_id += 1;
        self.register(opts.name.clone(), handle, opts.clone());
        Ok(())
    }
}
