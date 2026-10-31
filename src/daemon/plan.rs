//! Pure decisions behind starting a node. No process or socket I/O lives here.

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

/// Program and arguments for a pane started without a command: `default-command` run through the
/// default shell with `-c`, or the default shell alone. The shell is the configured `default-shell`,
/// else `$SHELL`, else `sh`.
pub fn default_argv(
    shell: Option<&str>,
    env_shell: Option<&str>,
    command: Option<&str>,
) -> (String, Vec<String>) {
    let shell = shell
        .or(env_shell)
        .filter(|s| !s.is_empty())
        .unwrap_or(if cfg!(windows) { "powershell.exe" } else { "/bin/sh" })
        .to_string();
    match command {
        Some(command) => (shell, vec!["-c".to_string(), command.to_string()]),
        None => (shell, Vec::new()),
    }
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

/// Environment for a pane: identity variables, then the spawn's own env (which wins on conflicts).
pub fn pane_env(
    pane: &PaneIdentity,
    extra: &[(String, String)],
) -> Vec<(String, String)> {
    let mut env = vec![
        ("MPX_ENDPOINT".to_string(), pane.endpoint.to_string()),
        ("MPX_SESSION".to_string(), pane.session.to_string()),
        ("MPX_PANE".to_string(), pane.name.to_string()),
        ("MPX_PANE_ID".to_string(), format!("%{}", pane.id)),
    ];
    env.extend(extra.iter().cloned());
    env
}
