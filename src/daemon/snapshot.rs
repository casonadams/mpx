//! Builds the persisted session snapshot from the workspace and the live panes. Pure: the caller
//! gathers the pane facts (live cwd, agent) and the clock.
use crate::layout::Workspace;
use crate::protocol::SpawnOpts;
use crate::store::{
    AgentSnapshot, PaneSnapshot, SNAPSHOT_VERSION, SessionSnapshot, WindowSnapshot,
};

/// What the snapshot knows about one pane in the layout.
#[derive(Debug, Clone)]
pub struct PaneRecord {
    pub name: String,
    /// The spawn options the pane was started with (user environment only).
    pub opts: SpawnOpts,
    pub running: bool,
    /// Live working directory when the snapshot was taken.
    pub cwd: Option<String>,
    pub agent: Option<AgentSnapshot>,
}

/// Snapshot of the running panes. Exited panes leave the layout, windows without running panes are
/// dropped, and the active window follows its window when it survives.
pub fn build_snapshot(
    session: &str,
    saved_ms: u64,
    clean_shutdown: bool,
    ws: &Workspace,
    panes: &[PaneRecord],
) -> SessionSnapshot {
    let live = |name: &str| panes.iter().find(|p| p.name == name && p.running);
    let mut windows = Vec::new();
    let mut active_window = None;
    let mut last_window = None;
    for (idx, win) in ws.windows.iter().enumerate() {
        let mut root = win.root.clone();
        for leaf in win.leaves() {
            if live(&leaf).is_none() {
                root = root.and_then(|r| r.remove_leaf(&leaf));
            }
        }
        let Some(layout) = root else { continue };
        let names = layout.all_leaves();
        let focused = win.focused.clone().filter(|f| names.contains(f));
        let window_panes = names
            .iter()
            .filter_map(|n| live(n))
            .map(pane_snapshot)
            .collect();
        if idx == ws.active_idx {
            active_window = Some(windows.len());
        }
        if Some(idx) == ws.last_window {
            last_window = Some(windows.len());
        }
        windows.push(WindowSnapshot {
            name: win.name.clone(),
            renamed: win.renamed,
            layout,
            focused,
            panes: window_panes,
        });
    }
    SessionSnapshot {
        version: SNAPSHOT_VERSION,
        session: session.to_string(),
        saved_ms,
        clean_shutdown,
        active_window: active_window.unwrap_or(0),
        last_window,
        windows,
    }
}

fn pane_snapshot(pane: &PaneRecord) -> PaneSnapshot {
    PaneSnapshot {
        name: pane.name.clone(),
        command: pane.opts.command.clone(),
        args: pane.opts.args.clone(),
        pty: pane.opts.pty,
        cwd: pane.cwd.clone().or_else(|| pane.opts.cwd.clone()),
        env: pane.opts.env.clone(),
        remain_on_exit: pane.opts.remain_on_exit,
        tmux_compat: pane.opts.tmux_compat,
        agent: pane.agent.clone(),
    }
}
