use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, MutexGuard};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::warn;

use super::DaemonState;
use super::plan::{self, PaneIdentity};
use crate::agents;
use crate::layout::{Direction, Workspace};
use crate::node::agent::AgentTracker;
use crate::node::process::{self, Identity, detection_names, foreground_name};
use crate::node::pty::PtyNode;
use crate::node::task::TaskNode;
use crate::node::{Node, SpawnSpec, now_ms};
use crate::pipe::VirtualPipe;
use crate::protocol::{NodeInfo, PaneId, SpawnOpts, StreamEvent};
use crate::tmux_shim;
use crate::vt::Alert;

pub(super) const KILL_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct Entry {
    pub id: PaneId,
    pub node: Arc<dyn Node>,
    pub remain: bool,
    pub agent: AgentTracker,
    /// Exit has been processed (layout, history, event).
    pub reported: bool,
}

pub(crate) struct Inner {
    /// Live nodes and exited nodes kept on screen (`remain_on_exit`).
    pub nodes: HashMap<String, Entry>,
    /// Exited nodes removed from the layout; oldest first, capped by `history_limit`.
    pub history: VecDeque<Entry>,
    pub workspace: Workspace,
    pub pipes: HashMap<String, VirtualPipe>,
    pub next_pane_id: PaneId,
}

impl Inner {
    pub fn new(session: String) -> Self {
        Self {
            nodes: HashMap::new(),
            history: VecDeque::new(),
            workspace: Workspace::new(session),
            pipes: HashMap::new(),
            next_pane_id: 0,
        }
    }
}

/// Where a new node goes in the layout.
pub(crate) enum Placement {
    /// Split the focused pane of the active window (new window when empty).
    Spawn,
    /// Split `target` (default: focused pane of the active window).
    Split {
        target: Option<String>,
        dir: Direction,
        percent: Option<u8>,
        focus: bool,
    },
    /// New window.
    Window { label: String, focus: bool },
}

pub(super) fn not_found(name: &str) -> String {
    format!("Node '{}' not found", name)
}

/// Identity of the process the pane is running (foreground, else the spawned process).
fn fg_identity(node: &dyn Node) -> Option<Identity> {
    node.foreground_pid().or(node.pid()).map(process::identity)
}

/// Entry for `name`: live nodes first, then the newest history entry.
pub(super) fn find_entry_mut<'a>(inner: &'a mut Inner, name: &str) -> Option<&'a mut Entry> {
    if inner.nodes.contains_key(name) {
        return inner.nodes.get_mut(name);
    }
    inner
        .history
        .iter_mut()
        .rev()
        .find(|e| e.node.name() == name)
}

fn is_history_name(inner: &Inner, name: &str) -> bool {
    inner.history.iter().any(|e| e.node.name() == name)
}

fn spawn_watcher(state: &Arc<DaemonState>, node: Arc<dyn Node>, id: PaneId) {
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let name = node.name().to_string();
        let _ = node.wait_exit().await;
        state.finish(&name, id);
    });
}

impl DaemonState {
    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Node name for a name or `%N` target; live nodes win over history.
    pub(super) fn resolve(inner: &Inner, target: &str) -> Option<String> {
        if let Some(num) = target.strip_prefix('%') {
            let id: PaneId = num.parse().ok()?;
            let live = inner.nodes.values().find(|e| e.id == id);
            let found = live.or_else(|| inner.history.iter().rev().find(|e| e.id == id))?;
            return Some(found.node.name().to_string());
        }
        if inner.nodes.contains_key(target) || is_history_name(inner, target) {
            return Some(target.to_string());
        }
        None
    }

    /// Runs `f` on the entry for `target`, live or exited.
    pub(super) fn with_entry<T>(
        &self,
        target: &str,
        f: impl FnOnce(&mut Entry) -> T,
    ) -> Result<T, String> {
        let mut inner = self.lock();
        let name = Self::resolve(&inner, target).ok_or_else(|| not_found(target))?;
        let entry = find_entry_mut(&mut inner, &name).ok_or_else(|| not_found(target))?;
        Ok(f(entry))
    }

    /// Runs `f` on a live node only.
    pub(super) fn with_live<T>(
        &self,
        target: &str,
        f: impl FnOnce(&mut Entry) -> T,
    ) -> Result<T, String> {
        self.with_entry(target, |entry| entry.node.is_running().then(|| f(entry)))?
            .ok_or_else(|| "Node has exited".to_string())
    }

    /// Resolves a target name, returning the canonical node name.
    pub(super) fn target_name(&self, target: &str) -> Result<String, String> {
        Self::resolve(&self.lock(), target).ok_or_else(|| not_found(target))
    }

    /// Applies pending terminal alerts and re-derives agent state for one entry.
    pub(super) fn refresh(&self, entry: &mut Entry, now: u64) {
        let alerts = entry.node.take_alerts();
        self.apply(entry, now, alerts);
    }

    fn apply(&self, entry: &mut Entry, now: u64, alerts: Vec<Alert>) {
        if !entry.node.is_running() {
            entry.agent.exited(now);
            return;
        }
        let names = fg_identity(entry.node.as_ref())
            .map(|id| detection_names(&id))
            .unwrap_or_default();
        let harness = names
            .iter()
            .find_map(|n| agents::detect(&self.harnesses, n))
            .map(|h| h.name.as_str());
        entry.agent.observe_foreground(harness, now);
        for alert in alerts {
            match alert {
                Alert::Bell => entry.agent.alert(None, now),
                Alert::Notify(msg) => entry.agent.alert(Some(msg), now),
            }
        }
        entry
            .agent
            .tick(now, entry.node.last_output_ms(), self.config.agent.idle_ms);
    }

    /// Applies alerts the reader produced since the last drain; cheap when there are none.
    pub(super) fn drain_alerts(&self, name: &str) {
        let mut inner = self.lock();
        let Some(entry) = find_entry_mut(&mut inner, name) else {
            return;
        };
        let alerts = entry.node.take_alerts();
        if !alerts.is_empty() {
            self.apply(entry, now_ms(), alerts);
        }
    }

    pub(super) fn info_for(
        &self,
        entry: &mut Entry,
        window: Option<usize>,
        full: bool,
    ) -> NodeInfo {
        self.refresh(entry, now_ms());
        let mut info = entry.node.info();
        info.window = window;
        info.agent = entry.agent.info();
        if full {
            info.title = entry.node.title();
            info.foreground = entry
                .node
                .foreground_pid()
                .map(process::identity)
                .and_then(|id| foreground_name(&id));
            info.cwd = entry.node.cwd();
        }
        info
    }

    pub(super) fn info_by_name(&self, name: &str, full: bool) -> Result<NodeInfo, String> {
        let mut inner = self.lock();
        let window = inner.workspace.window_of(name);
        let entry = find_entry_mut(&mut inner, name).ok_or_else(|| not_found(name))?;
        Ok(self.info_for(entry, window, full))
    }

    pub(super) fn list_nodes(&self) -> Vec<NodeInfo> {
        let mut inner = self.lock();
        let Inner {
            nodes,
            history,
            workspace,
            ..
        } = &mut *inner;
        let mut list: Vec<NodeInfo> = nodes
            .iter_mut()
            .map(|(name, entry)| self.info_for(entry, workspace.window_of(name), true))
            .collect();
        list.extend(
            history
                .iter_mut()
                .map(|entry| self.info_for(entry, None, true)),
        );
        list
    }

    /// Refreshes automatic window names from the focused pane of each window.
    pub(super) fn refresh_window_names(&self, inner: &mut Inner) {
        let now = now_ms();
        for idx in 0..inner.workspace.windows.len() {
            let focused = inner.workspace.windows[idx].focused.clone();
            let label = focused.and_then(|f| {
                let entry = inner.nodes.get_mut(&f)?;
                self.refresh(entry, now);
                entry.agent.info().map(|a| a.harness).or_else(|| {
                    entry
                        .node
                        .foreground_pid()
                        .map(process::identity)
                        .and_then(|id| foreground_name(&id))
                })
            });
            inner.workspace.set_auto_name(idx, label);
        }
    }

    pub(super) fn session_info(&self) -> crate::protocol::SessionInfo {
        let inner = self.lock();
        crate::protocol::SessionInfo {
            name: self.session.clone(),
            endpoint: self.endpoint.clone(),
            pid: std::process::id(),
            created_ms: self.created_ms,
            windows: inner.workspace.windows.len(),
            panes: inner.workspace.pane_count(),
        }
    }

    fn pane_env(
        &self,
        identity: &PaneIdentity,
        extra: &[(String, String)],
        tmux: bool,
    ) -> Vec<(String, String)> {
        if !tmux {
            return plan::pane_env(identity, extra, None);
        }
        let shim = tmux_shim::shim_dir();
        if let Err(e) = &shim {
            warn!("tmux shim unavailable, PATH not updated: {}", e);
        }
        let inherited = std::env::var("PATH").unwrap_or_default();
        let tmux_env = plan::TmuxEnv {
            daemon_pid: std::process::id(),
            shim_dir: shim.as_deref().map_err(String::as_str),
            inherited_path: &inherited,
        };
        plan::pane_env(identity, extra, Some(&tmux_env))
    }

    fn check_placement(inner: &Inner, place: &Placement) -> Result<(), String> {
        match place {
            Placement::Split {
                target: Some(t), ..
            } if inner.workspace.window_of(t).is_none() => Err(not_found(t)),
            _ => Ok(()),
        }
    }

    fn place_node(inner: &mut Inner, name: &str, place: Placement) -> Result<(), String> {
        match place {
            Placement::Spawn => {
                inner.workspace.add_or_split_with_focus(
                    name.to_string(),
                    Direction::Horizontal,
                    true,
                );
                Ok(())
            }
            Placement::Split {
                target,
                dir,
                percent,
                focus,
            } => {
                inner
                    .workspace
                    .split_pane(target.as_deref(), name.to_string(), dir, percent, focus)
            }
            Placement::Window { label, focus } => {
                inner
                    .workspace
                    .new_window_with_focus(label, name.to_string(), focus);
                Ok(())
            }
        }
    }

    /// Starts a node and places it in the layout.
    pub(super) fn add_node(
        self: &Arc<Self>,
        opts: SpawnOpts,
        place: Placement,
    ) -> Result<NodeInfo, String> {
        let mut inner = self.lock();
        let id = inner.next_pane_id;
        let name = plan::spawn_name(&opts.name, &opts.command, id);
        plan::validate_name(&name)?;
        Self::check_new_pane(&inner, &name, &place)?;

        let anchor = Self::placement_anchor(&inner, &place);
        let cwd = opts
            .cwd
            .clone()
            .or_else(|| Self::anchor_cwd(&inner, anchor.as_deref()))
            .or_else(Self::current_dir);
        let identity = PaneIdentity {
            endpoint: &self.endpoint,
            session: &self.session,
            name: &name,
            id,
        };
        let tmux = plan::tmux_enabled(opts.tmux_compat, self.config.tmux_compat);
        let env = self.pane_env(&identity, &opts.env, tmux);
        let spec = SpawnSpec {
            name: name.clone(),
            id,
            command: opts.command,
            args: opts.args,
            cwd,
            env,
        };
        let node = Self::start_node(opts.pty, spec, &self.event_tx)?;
        Self::place_node(&mut inner, &name, place).inspect_err(|_| {
            let _ = node.kill();
        })?;

        inner.next_pane_id += 1;
        let remain = plan::remain_on_exit(opts.remain_on_exit, self.config.remain_on_exit);
        self.register(&mut inner, &name, id, node, remain)
    }

    fn check_new_pane(inner: &Inner, name: &str, place: &Placement) -> Result<(), String> {
        if inner.nodes.contains_key(name) {
            return Err(format!("Node '{}' already exists", name));
        }
        Self::check_placement(inner, place)
    }

    fn start_node(
        pty: bool,
        spec: SpawnSpec,
        events: &broadcast::Sender<StreamEvent>,
    ) -> Result<Arc<dyn Node>, String> {
        let node: Arc<dyn Node> = if pty {
            Arc::new(PtyNode::spawn(spec, events.clone()).map_err(|e| e.to_string())?)
        } else {
            Arc::new(TaskNode::spawn(spec, events.clone()).map_err(|e| e.to_string())?)
        };
        Ok(node)
    }

    fn placement_anchor(inner: &Inner, place: &Placement) -> Option<String> {
        let (is_spawn, target) = match place {
            Placement::Spawn => (true, None),
            Placement::Split { target, .. } => (false, target.as_deref()),
            Placement::Window { .. } => (false, None),
        };
        let focused = inner
            .workspace
            .active_window()
            .and_then(|w| w.focused.as_deref());
        plan::cwd_anchor(is_spawn, target, focused).map(str::to_string)
    }

    fn anchor_cwd(inner: &Inner, anchor: Option<&str>) -> Option<String> {
        anchor
            .and_then(|p| inner.nodes.get(p))
            .and_then(|e| e.node.cwd())
    }

    fn current_dir() -> Option<String> {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    fn register(
        self: &Arc<Self>,
        inner: &mut Inner,
        name: &str,
        id: PaneId,
        node: Arc<dyn Node>,
        remain: bool,
    ) -> Result<NodeInfo, String> {
        let _ = self.event_tx.send(StreamEvent::Spawned {
            name: name.to_string(),
            pid: node.pid(),
        });
        spawn_watcher(self, Arc::clone(&node), id);
        inner.nodes.insert(
            name.to_string(),
            Entry {
                id,
                node,
                remain,
                agent: AgentTracker::default(),
                reported: false,
            },
        );
        let window = inner.workspace.window_of(name);
        let entry = inner.nodes.get_mut(name).ok_or_else(|| not_found(name))?;
        Ok(self.info_for(entry, window, true))
    }

    /// Records an exit: keeps or removes the pane and emits the exit event. Idempotent.
    pub(super) fn finish(&self, name: &str, id: PaneId) {
        let mut inner = self.lock();
        let Some(entry) = inner.nodes.get_mut(name) else {
            return;
        };
        if entry.id != id || entry.reported || entry.node.is_running() {
            return;
        }
        entry.reported = true;
        entry.node.release();
        entry.agent.exited(now_ms());
        let code = entry.node.exit_code();
        if !entry.remain {
            if let Some(entry) = inner.nodes.remove(name) {
                inner.history.push_back(entry);
            }
            inner.workspace.remove(name);
            while inner.history.len() > self.config.history_limit {
                inner.history.pop_front();
            }
        }
        let _ = self.event_tx.send(StreamEvent::Exit {
            name: name.to_string(),
            code,
        });
    }

    /// Kills a node (or removes an exited one). Waits briefly for the process to exit.
    pub(super) async fn kill_node(&self, target: &str) -> Result<(), String> {
        let (node, id, name) = {
            let mut inner = self.lock();
            let name = Self::resolve(&inner, target).ok_or_else(|| not_found(target))?;
            match inner
                .nodes
                .get(&name)
                .map(|e| (e.node.is_running(), Arc::clone(&e.node), e.id))
            {
                Some((true, node, id)) => (node, id, name),
                Some((false, _, _)) => {
                    inner.nodes.remove(&name);
                    inner.workspace.remove(&name);
                    return Ok(());
                }
                None => {
                    if let Some(pos) = inner.history.iter().rposition(|e| e.node.name() == name) {
                        inner.history.remove(pos);
                    }
                    return Ok(());
                }
            }
        };
        let _ = node.kill();
        let _ = tokio::time::timeout(KILL_GRACE, node.wait_exit()).await;
        self.finish(&name, id);
        Ok(())
    }

    pub(super) async fn kill_window(&self, index: Option<usize>) -> Result<(), String> {
        let panes = {
            let mut inner = self.lock();
            let idx = index.unwrap_or(inner.workspace.active_idx);
            inner
                .workspace
                .kill_window(idx)
                .ok_or_else(|| "Window index out of bounds".to_string())?
        };
        for pane in panes {
            let _ = self.kill_node(&pane).await;
        }
        Ok(())
    }

    /// Processes the exit of `name` if its process has ended (no-op otherwise).
    pub(super) fn settle(&self, name: &str) {
        let id = self.lock().nodes.get(name).map(|e| e.id);
        if let Some(id) = id {
            self.finish(name, id);
        }
    }

    /// Builds bytes for a live node, sends them and marks any pending prompt as answered.
    pub(super) fn send_with(
        &self,
        target: &str,
        build: impl FnOnce(&dyn Node) -> Vec<u8>,
    ) -> Result<(), String> {
        self.with_live(target, |entry| {
            let bytes = build(entry.node.as_ref());
            entry
                .node
                .send_input(&bytes)
                .map(|()| entry.agent.input(now_ms()))
                .map_err(|e| e.to_string())
        })?
    }

    /// Kills every running node and waits briefly for them to exit (daemon shutdown).
    pub(super) async fn stop_nodes(&self) {
        let running: Vec<Arc<dyn Node>> = self
            .lock()
            .nodes
            .values()
            .filter(|e| e.node.is_running())
            .map(|e| Arc::clone(&e.node))
            .collect();
        for node in &running {
            let _ = node.kill();
        }
        let _ = tokio::time::timeout(KILL_GRACE, async {
            for node in &running {
                node.wait_exit().await;
            }
        })
        .await;
    }
}
