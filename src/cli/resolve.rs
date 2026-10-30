use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::ipc::endpoint_in;
use crate::protocol::{WaitCondition, WaitOutcome};
use crate::store;

/// Exit code of `mpx wait` when the timeout elapses first (same as coreutils `timeout`).
pub const WAIT_TIMEOUT_EXIT: i32 = 124;

/// Session a bare `mpx` attaches to, named as tmux names its first session.
pub const DEFAULT_SESSION: &str = "0";

/// What the default session was called before it became `DEFAULT_SESSION`.
pub const LEGACY_DEFAULT_SESSION: &str = "main";

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub session: String,
    pub endpoint: String,
    /// Directory holding the session sockets (and the runtime files mpx writes next to them).
    pub socket_dir: PathBuf,
    /// Applied to the next mutating request of the command (`--idempotency-key`).
    pub idempotency_key: Option<String>,
    /// Config file given with `-f`; an auto-spawned daemon loads the same file.
    pub config_path: Option<PathBuf>,
    /// Config the daemon and the agent/MCP commands use.
    pub config: Config,
    /// Durable state directory of the session's daemon (`--state-dir`).
    pub state_dir: PathBuf,
    /// `--no-restore`: start the session empty instead of from its saved snapshot.
    pub no_restore: bool,
}

/// Session: `-s` > `$MPX_SESSION` > `0`. Endpoint: `--endpoint` > `$MPX_ENDPOINT` (only when `-s` is absent) > the session's socket in `socket_dir`.
/// The config is the default until the caller loads one with `load_config`.
pub fn resolve(
    session_flag: Option<&str>,
    endpoint_flag: Option<&str>,
    env_session: Option<&str>,
    env_endpoint: Option<&str>,
    socket_dir: &Path,
) -> Resolved {
    let session = session_flag.or(env_session).unwrap_or(DEFAULT_SESSION).to_string();
    let env_ep = if session_flag.is_none() {
        env_endpoint
    } else {
        None
    };
    let endpoint = endpoint_flag
        .or(env_ep)
        .map(str::to_string)
        .unwrap_or_else(|| endpoint_in(socket_dir, &session));
    let state_dir = store::session_dir(&session);
    Resolved {
        session,
        endpoint,
        socket_dir: socket_dir.to_path_buf(),
        idempotency_key: None,
        config_path: None,
        config: Config::default(),
        state_dir,
        no_restore: false,
    }
}

/// Config from the `-f` file when given, else from the default location. A missing file is defaults;
/// a malformed `-f` file is an error, while a malformed default file only logs a warning.
pub fn load_config(path: Option<&Path>) -> Result<Config, String> {
    match path {
        Some(path) => Config::load_file(path),
        None => Ok(Config::load_or_default()),
    }
}

/// Idempotency key from `--idempotency-key`; an empty value is unset. Deliberately not read from the
/// environment: an exported key would silently dedupe every later mutating call in that shell.
pub fn resolve_idempotency_key(flag: Option<&str>) -> Option<String> {
    flag.filter(|s| !s.is_empty()).map(str::to_string)
}

/// Attaching from inside an mpx pane nests the TUI, which tmux also refuses unless forced.
pub fn nested_attach_refused(inside_mpx: bool, force: bool) -> bool {
    inside_mpx && !force
}

/// Parses repeatable `K=V` environment arguments.
pub fn parse_env_pairs(items: &[String]) -> Result<Vec<(String, String)>, String> {
    items
        .iter()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("invalid environment entry (expected K=V): {kv}"))
        })
        .collect()
}

/// Condition selected by `--idle` or `--match`; none means process exit.
pub fn wait_condition(
    idle: Option<u64>,
    pattern: Option<&str>,
) -> Result<WaitCondition, String> {
    if idle.is_some() && pattern.is_some() {
        return Err("use only one of --idle, --match".to_string());
    }
    if let Some(quiet_ms) = idle {
        return Ok(WaitCondition::Idle { quiet_ms });
    }
    if let Some(pattern) = pattern {
        regex::Regex::new(pattern).map_err(|e| format!("invalid --match regex: {e}"))?;
        return Ok(WaitCondition::Match {
            pattern: pattern.to_string(),
        });
    }
    Ok(WaitCondition::Exit)
}

/// Process exit status for `mpx wait`: the child's code for exit waits, 0 when another condition holds, 124 on timeout.
pub fn wait_exit_code(condition: &WaitCondition, outcome: &WaitOutcome) -> i32 {
    if !outcome.satisfied {
        return WAIT_TIMEOUT_EXIT;
    }
    match condition {
        WaitCondition::Exit => outcome.exit_code.unwrap_or(0),
        _ => 0,
    }
}

/// Lowest number that names none of `taken`: how tmux names a session created without `-s`.
pub fn next_free_session<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: std::collections::HashSet<&str> = taken.into_iter().collect();
    let mut n = 0u64;
    while taken.contains(n.to_string().as_str()) {
        n += 1;
    }
    n.to_string()
}
