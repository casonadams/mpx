use std::path::Path;

use crate::agents::{self, Harness, LaunchOptions, LaunchSpec, find_harness};
use crate::protocol::SpawnOpts;
use crate::store::{PaneSnapshot, RestorePlan, SessionSnapshot, WindowPlan, WindowSnapshot};

/// Restores PTY panes with their original command in their last cwd; agents with a known session id
/// are resumed through their harness, other agents start fresh; non-PTY task panes are not restored.
/// Windows left without panes are dropped. Harness runtime files are written under `runtime_dir`.
pub fn plan_restore(snapshot: &SessionSnapshot, harnesses: &[Harness], runtime_dir: &Path) -> RestorePlan {
    plan_restore_with(snapshot, harnesses, |h, resume, opts| {
        agents::launch(h, resume, opts, runtime_dir)
    })
}

/// `plan_restore` with the agent launcher injected, so the plan is computed without I/O.
pub fn plan_restore_with<F>(
    snapshot: &SessionSnapshot,
    harnesses: &[Harness],
    launcher: F,
) -> RestorePlan
where
    F: Fn(&Harness, Option<&str>, LaunchOptions) -> LaunchSpec,
{
    let planned: Vec<Option<WindowPlan>> = snapshot
        .windows
        .iter()
        .map(|window| plan_window(window, harnesses, &launcher))
        .collect();
    RestorePlan {
        active_window: remap_active(snapshot.active_window, &planned),
        windows: planned.into_iter().flatten().collect(),
    }
}

/// Index among surviving windows of the active window, or of the next survivor when it was dropped.
fn remap_active(active: usize, planned: &[Option<WindowPlan>]) -> usize {
    let survivors = planned.iter().filter(|w| w.is_some()).count();
    let before = planned.iter().take(active).filter(|w| w.is_some()).count();
    before.min(survivors.saturating_sub(1))
}

fn plan_window<F>(
    window: &WindowSnapshot,
    harnesses: &[Harness],
    launcher: &F,
) -> Option<WindowPlan>
where
    F: Fn(&Harness, Option<&str>, LaunchOptions) -> LaunchSpec,
{
    let panes: Vec<SpawnOpts> = window
        .panes
        .iter()
        .filter(|pane| pane.pty)
        .map(|pane| plan_pane(pane, harnesses, launcher))
        .collect();
    if panes.is_empty() {
        return None;
    }
    let mut layout = window.layout.clone();
    for leaf in window.layout.all_leaves() {
        if !panes.iter().any(|pane| pane.name == leaf) {
            layout = layout.remove_leaf(&leaf)?;
        }
    }
    let leaves = layout.all_leaves();
    let focused = window
        .focused
        .clone()
        .filter(|name| leaves.contains(name))
        .or_else(|| leaves.first().cloned());
    Some(WindowPlan {
        name: window.renamed.then(|| window.name.clone()),
        layout,
        focused,
        panes,
    })
}

fn plan_pane<F>(pane: &PaneSnapshot, harnesses: &[Harness], launcher: &F) -> SpawnOpts
where
    F: Fn(&Harness, Option<&str>, LaunchOptions) -> LaunchSpec,
{
    let original = SpawnOpts {
        name: pane.name.clone(),
        command: pane.command.clone(),
        args: pane.args.clone(),
        pty: true,
        cwd: pane.cwd.clone(),
        env: pane.env.clone(),
        remain_on_exit: pane.remain_on_exit,
        tmux_compat: pane.tmux_compat,
    };
    let Some(agent) = &pane.agent else {
        return original;
    };
    let Some(harness) = find_harness(harnesses, &agent.harness) else {
        return original;
    };
    let resume = agent
        .session_id
        .as_deref()
        .filter(|_| !harness.resume.is_empty());
    let spec = launcher(
        harness,
        resume,
        LaunchOptions {
            teams: pane.tmux_compat,
        },
    );
    match spec.argv.split_first() {
        Some((command, args)) => SpawnOpts {
            command: command.clone(),
            args: args.to_vec(),
            env: spec
                .env
                .into_iter()
                .chain(pane.env.iter().cloned())
                .collect(),
            tmux_compat: spec.tmux_compat,
            ..original
        },
        None => original,
    }
}
