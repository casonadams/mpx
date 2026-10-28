//! Pure decisions behind starting a node. No process or socket I/O lives here.

use std::path::Path;

use crate::node::process::basename;
use crate::protocol::PaneId;

/// Identity of a pane as seen by the process it starts.
#[derive(Debug, Clone, Copy)]
pub struct PaneIdentity<'a> {
    pub endpoint: &'a str,
    pub session: &'a str,
    pub name: &'a str,
    pub id: PaneId,
}

/// Inputs for tmux compatibility. `shim_dir` is the outcome of locating the `tmux` shim.
#[derive(Debug, Clone, Copy)]
pub struct TmuxEnv<'a> {
    pub daemon_pid: u32,
    pub shim_dir: Result<&'a Path, &'a str>,
    pub inherited_path: &'a str,
}

/// Requested name, or `<command basename>-<id>` when none was requested.
pub fn spawn_name(requested: &str, command: &str, id: PaneId) -> String {
    if requested.is_empty() {
        format!("{}-{}", basename(command), id)
    } else {
        requested.to_string()
    }
}

/// Names must be non-empty and must not look like `%N` pane references.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.starts_with('%') {
        return Err(format!("Invalid node name '{}'", name));
    }
    Ok(())
}

/// The request wins over the daemon config default.
pub fn remain_on_exit(requested: Option<bool>, config_default: bool) -> bool {
    requested.unwrap_or(config_default)
}

/// tmux compatibility is on when either the spawn or the daemon config asks for it.
pub fn tmux_enabled(spawn_requested: bool, config_default: bool) -> bool {
    spawn_requested || config_default
}

/// Pane whose live working directory a new node inherits: none for a plain spawn, the split
/// target when given, otherwise the focused pane.
pub fn cwd_anchor<'a>(
    is_spawn: bool,
    split_target: Option<&'a str>,
    focused: Option<&'a str>,
) -> Option<&'a str> {
    if is_spawn {
        None
    } else {
        split_target.or(focused)
    }
}

/// `PATH` with the shim directory first; `None` when the shim is unavailable.
pub fn shim_path(tmux: &TmuxEnv) -> Option<String> {
    let dir = tmux.shim_dir.ok()?.to_string_lossy().into_owned();
    Some(if tmux.inherited_path.is_empty() {
        dir
    } else {
        format!("{}:{}", dir, tmux.inherited_path)
    })
}

fn tmux_vars(pane: &PaneIdentity, pane_ref: &str, tmux: &TmuxEnv) -> Vec<(String, String)> {
    let mut vars = vec![
        (
            "TMUX".to_string(),
            format!("{},{},0", pane.endpoint, tmux.daemon_pid),
        ),
        ("TMUX_PANE".to_string(), pane_ref.to_string()),
    ];
    if let Some(path) = shim_path(tmux) {
        vars.push(("PATH".to_string(), path));
    }
    vars
}

/// Environment for a pane: identity variables, then tmux variables, then the spawn's own env
/// (which wins on conflicts).
pub fn pane_env(
    pane: &PaneIdentity,
    extra: &[(String, String)],
    tmux: Option<&TmuxEnv>,
) -> Vec<(String, String)> {
    let pane_ref = format!("%{}", pane.id);
    let mut env = vec![
        ("MPX_ENDPOINT".to_string(), pane.endpoint.to_string()),
        ("MPX_SESSION".to_string(), pane.session.to_string()),
        ("MPX_PANE".to_string(), pane.name.to_string()),
        ("MPX_PANE_ID".to_string(), pane_ref.clone()),
    ];
    if let Some(tmux) = tmux {
        env.extend(tmux_vars(pane, &pane_ref, tmux));
    }
    env.extend(extra.iter().cloned());
    env
}
