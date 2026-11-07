//! Pane exit and kill. A kill starts the end of a running process; an exit keeps the pane on screen
//! or removes it from the layout, and a removed pane's entry goes to the bounded history.

use tokio::sync::oneshot;

use super::{Origin, Reply, Session};
use crate::layout::window_not_found;
use crate::protocol::{PaneId, StreamEvent};

impl Session {
    /// Starts a kill for a client or a hook. A client's kill that worked fires `after-kill-pane`.
    pub(super) fn kill_msg(
        &mut self,
        target: &str,
        origin: Origin,
        reply: Reply<Result<Option<oneshot::Receiver<()>>, String>>,
    ) {
        let killed = self.target_name(target).ok();
        let result = self.kill(target);
        self.schedule_save();
        if let Some(name) = killed
            && origin == Origin::Client
            && result.is_ok()
        {
            self.fire("after-kill-pane", Some(name));
        }
        let _ = reply.send(result);
    }

    /// A pane's process ended: its exit fires the hooks and reaches the subscribers.
    pub(super) fn pane_exited(&mut self, name: String, id: PaneId, code: Option<i32>) {
        if self.finish(&name, id, code) {
            self.fire("pane-exited", Some(name.clone()));
            if self.nodes.contains_key(&name) {
                self.fire("pane-died", Some(name.clone()));
            }
            self.fanout(StreamEvent::Exit {
                name: name.clone(),
                code,
            });
        }
    }

    /// Records an exit: keeps or removes the pane. Ignores stale or repeated notices; returns
    /// whether the exit belonged to the pane's current process.
    fn finish(&mut self, name: &str, id: PaneId, code: Option<i32>) -> bool {
        let Some(entry) = self.nodes.get_mut(name) else {
            return false;
        };
        if entry.handle.id() != id {
            return false;
        }
        entry.exited = true;
        let Some(entry) = self.nodes.get(name) else {
            return true;
        };
        let keep = entry
            .opts
            .remain_on_exit
            .unwrap_or_else(|| self.config.remain_on_exit.keeps(code));
        if !keep {
            if let Some(entry) = self.nodes.remove(name) {
                self.history.push_back(entry);
            }
            self.workspace.remove(name);
            while self.history.len() > self.config.session.history_limit {
                self.history.pop_front();
            }
        }
        self.schedule_save();
        true
    }

    /// Starts a kill of a live pane (the receiver resolves on exit), or removes an exited one.
    fn kill(&mut self, target: &str) -> Result<Option<oneshot::Receiver<()>>, String> {
        let name = self.target_name(target)?;
        Ok(self.kill_entry(&name))
    }

    /// Starts a kill of pane `name`, or removes it when it has exited.
    pub(super) fn kill_entry(&mut self, name: &str) -> Option<oneshot::Receiver<()>> {
        if let Some(entry) = self.nodes.get(name) {
            if !entry.exited {
                return Some(entry.handle.request_kill());
            }
            self.nodes.remove(name);
            self.workspace.remove(name);
            return None;
        }
        if let Some(pos) = self.history.iter().rposition(|e| e.handle.name() == name) {
            self.history.remove(pos);
        }
        None
    }

    pub(super) fn kill_window(
        &mut self,
        index: Option<usize>,
    ) -> Result<Vec<oneshot::Receiver<()>>, String> {
        let number = self.window_index(index)?;
        let panes = self
            .workspace
            .kill_window(number)
            .ok_or_else(|| window_not_found(number))?;
        Ok(panes.iter().filter_map(|p| self.kill_entry(p)).collect())
    }

    /// Saves a clean snapshot, then starts killing every running pane. Later saves are disabled.
    pub(super) async fn shutdown(&mut self) -> Vec<oneshot::Receiver<()>> {
        self.save(true).await;
        self.shutting_down = true;
        self.save_due = None;
        self.nodes
            .values()
            .filter(|e| !e.exited)
            .map(|e| e.handle.request_kill())
            .collect()
    }
}
