//! The pane registry: resolving names and ids to live or exited panes, their handles and info.

use futures_util::future::join_all;

use super::Entry;
use super::Session;
use super::answer::not_found;
use crate::node::pane::PaneHandle;
use crate::protocol::{NodeInfo, PaneId, SpawnOpts, StreamEvent};

impl Session {
    pub(super) fn active_pane(&self) -> Option<String> {
        self.workspace
            .active_window()
            .and_then(|w| w.focused.clone())
    }

    /// The named pane, or the active window's focused pane when `name` is None.
    pub(super) fn pane_or_active(&self, name: Option<String>) -> Result<String, String> {
        match name {
            Some(n) => self.target_name(&n),
            None => self
                .active_pane()
                .ok_or_else(|| "No active pane".to_string()),
        }
    }

    /// The name a target (a pane name or `%id`) denotes, when a live or exited pane has it.
    fn resolve(&self, target: &str) -> Option<String> {
        if let Some(num) = target.strip_prefix('%') {
            let id: PaneId = num.parse().ok()?;
            let live = self.nodes.values().find(|e| e.handle.id() == id);
            let found = live.or_else(|| self.history.iter().rev().find(|e| e.handle.id() == id))?;
            return Some(found.handle.name().to_string());
        }
        let known = self.nodes.contains_key(target)
            || self.history.iter().any(|e| e.handle.name() == target);
        known.then(|| target.to_string())
    }

    pub(super) fn target_name(&self, target: &str) -> Result<String, String> {
        self.resolve(target).ok_or_else(|| not_found(target))
    }

    /// Handle of a live or exited pane.
    pub(super) fn handle_of(&self, target: &str) -> Result<PaneHandle, String> {
        let name = self.target_name(target)?;
        self.nodes
            .get(&name)
            .or_else(|| {
                self.history
                    .iter()
                    .rev()
                    .find(|e| e.handle.name() == name.as_str())
            })
            .map(|e| e.handle.clone())
            .ok_or_else(|| not_found(target))
    }

    pub(super) async fn info_of(&self, name: &str, full: bool) -> Result<NodeInfo, String> {
        let handle = self.handle_of(name)?;
        let mut info = handle.info(full).await?;
        self.annotate(&mut info, name, true);
        Ok(info)
    }

    /// Adds what the session knows about pane `name` to its info: its window and input state when
    /// it is in the layout, and whether it is marked.
    fn annotate(&self, info: &mut NodeInfo, name: &str, in_layout: bool) {
        if in_layout {
            info.window = self.workspace.number_of(name);
            info.input_off = Some(self.workspace.input_off(name));
        }
        info.marked = Some(self.workspace.marked_pane.as_deref() == Some(name));
    }

    pub(super) async fn status(&self, name: &str) -> Result<NodeInfo, String> {
        let name = self.target_name(name)?;
        self.info_of(&name, true).await
    }

    /// Every pane's info, asked of all the panes at once. History holds exited panes that left
    /// the layout; a live pane may reuse their name.
    pub(super) async fn list(&self) -> Vec<NodeInfo> {
        let live = self.nodes.values().map(|entry| (entry, true));
        let exited = self.history.iter().map(|entry| (entry, false));
        let infos = join_all(live.chain(exited).map(|(entry, in_layout)| async move {
            let mut info = entry.handle.info(true).await.ok()?;
            self.annotate(&mut info, entry.handle.name(), in_layout);
            Some(info)
        }))
        .await;
        let mut infos: Vec<NodeInfo> = infos.into_iter().flatten().collect();
        infos.sort_by_key(|i| i.id);
        infos
    }

    /// Records a started pane in the registry and tells the subscribers it exists.
    pub(super) fn register(&mut self, name: String, handle: PaneHandle, opts: SpawnOpts) {
        self.fanout(StreamEvent::Spawned {
            name: name.clone(),
            pid: handle.pid(),
        });
        self.attach_pending(&name, &handle);
        self.nodes.insert(
            name,
            Entry {
                handle,
                opts,
                exited: false,
            },
        );
    }
}
