//! `tmux` compatibility shim: the mpx binary invoked as `tmux` translates a tmux CLI subset to mpx requests.
use std::path::{Path, PathBuf};

mod args;
mod conf;
mod exec;
mod extra;
mod format;
mod server;
mod shell;

pub use args::{Invocation, canonical, is_noop, parse as parse_invocation, spec};
pub use conf::{Converted, convert};
pub use exec::{EnvTarget, neighbor, run_in, pane_key, pane_view, run, side_of, spawn_opts, split_direction, window_key};
pub use extra::{buffer_sample, builtin_option, quote_option_value};
pub use format::{PaneView, Vars, expand, pane_vars, session_vars};
pub use server::{default_session, server_key};
pub use shell::{command_line, config_lines, join_words, split_commands};

/// Endpoint of the mpx server a tmux command should talk to: `$TMUX` (`endpoint,pid,idx`) first, then `$MPX_ENDPOINT`.
pub fn endpoint_from_env(tmux: Option<&str>, mpx_endpoint: Option<&str>) -> Option<String> {
    tmux.and_then(|t| t.split(',').next())
        .filter(|s| !s.is_empty())
        .or(mpx_endpoint)
        .map(str::to_string)
}

/// Session name for an endpoint (its socket or pipe file stem).
pub fn session_of(endpoint: &str) -> String {
    let stem = Path::new(endpoint)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.strip_prefix("mpx-")
        .map(str::to_string)
        .unwrap_or(stem)
}

/// Directory holding a `tmux` link to the current mpx executable; created on first use.
pub fn shim_dir() -> Result<PathBuf, String> {
    let dir = crate::ipc::socket_dir().join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    install_tmux_link(&dir, &exe)?;
    Ok(dir)
}

#[cfg(unix)]
fn install_tmux_link(dir: &Path, exe: &Path) -> Result<(), String> {
    let link = dir.join("tmux");
    if std::fs::read_link(&link).is_ok_and(|target| target == exe) {
        return Ok(());
    }
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(exe, &link).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn install_tmux_link(dir: &Path, exe: &Path) -> Result<(), String> {
    let link = dir.join("tmux.exe");
    let up_to_date = match (std::fs::metadata(&link), std::fs::metadata(exe)) {
        (Ok(a), Ok(b)) => a.len() == b.len(),
        _ => false,
    };
    if up_to_date {
        return Ok(());
    }
    std::fs::copy(exe, &link)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
