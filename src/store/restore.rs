use crate::config::ResumeRule;
use crate::protocol::SpawnOpts;
use crate::store::{PaneSnapshot, RestorePlan, SessionSnapshot, WindowPlan, WindowSnapshot};

/// Restores PTY panes with their original command or configured resume command in their last cwd;
/// non-PTY task panes are not restored. Windows left without panes are dropped.
pub fn plan_restore(
    snapshot: &SessionSnapshot,
    rules: &[ResumeRule],
) -> RestorePlan {
    let planned: Vec<Option<WindowPlan>> = snapshot
        .windows
        .iter()
        .map(|window| plan_window(window, rules))
        .collect();
    RestorePlan {
        active_window: remap_active(snapshot.active_window, &planned),
        last_window: remap_last(snapshot.last_window, &planned),
        windows: planned.into_iter().flatten().collect(),
    }
}

/// Index among surviving windows of the last active window, or None when dropped.
fn remap_last(last: Option<usize>, planned: &[Option<WindowPlan>]) -> Option<usize> {
    let last = last?;
    let mut mapped = 0;
    for (i, p) in planned.iter().enumerate() {
        if i == last {
            return p.as_ref().map(|_| mapped);
        }
        if p.is_some() {
            mapped += 1;
        }
    }
    None
}

/// Index among surviving windows of the active window, or of the next survivor when it was dropped.
fn remap_active(active: usize, planned: &[Option<WindowPlan>]) -> usize {
    let survivors = planned.iter().filter(|w| w.is_some()).count();
    let before = planned.iter().take(active).filter(|w| w.is_some()).count();
    before.min(survivors.saturating_sub(1))
}

fn plan_window(
    window: &WindowSnapshot,
    rules: &[ResumeRule],
) -> Option<WindowPlan> {
    let panes: Vec<SpawnOpts> = window
        .panes
        .iter()
        .filter(|pane| pane.pty)
        .map(|pane| plan_pane(pane, rules))
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

fn find_rule<'a>(rules: &'a [ResumeRule], name_or_cmd: &str) -> Option<&'a ResumeRule> {
    rules.iter().find(|r| {
        r.name.eq_ignore_ascii_case(name_or_cmd)
            || r.detect.iter().any(|d| d.eq_ignore_ascii_case(name_or_cmd))
    })
}

fn fill(template: &[String], id: &str) -> Vec<String> {
    template.iter().map(|s| s.replace("{id}", id)).collect()
}

fn plan_pane(pane: &PaneSnapshot, rules: &[ResumeRule]) -> SpawnOpts {
    let original = SpawnOpts {
        name: pane.name.clone(),
        command: pane.command.clone(),
        args: pane.args.clone(),
        pty: true,
        cwd: pane.cwd.clone(),
        env: pane.env.clone(),
        remain_on_exit: pane.remain_on_exit,
    };

    let rule = pane
        .resume_rule
        .as_deref()
        .and_then(|name| find_rule(rules, name))
        .or_else(|| find_rule(rules, &pane.command));

    let Some(rule) = rule else {
        return original;
    };

    let argv = if let Some(id) = pane.session_id.as_deref().filter(|_| !rule.resume.is_empty()) {
        fill(&rule.resume, id)
    } else if !rule.fallback.is_empty() {
        rule.fallback.clone()
    } else {
        return original;
    };

    match argv.split_first() {
        Some((cmd, args)) => SpawnOpts {
            command: cmd.clone(),
            args: args.to_vec(),
            ..original
        },
        None => original,
    }
}
