use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::ipc::endpoint_in;
use crate::protocol::{AgentState, WaitCondition, WaitOutcome};
use crate::store;

/// Exit code of `mpx wait` when the timeout elapses first (same as coreutils `timeout`).
pub const WAIT_TIMEOUT_EXIT: i32 = 124;

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
}

/// Session: `-s` > `$MPX_SESSION` > `main`. Endpoint: `--endpoint` > `$MPX_ENDPOINT` (only when `-s` is absent) > the session's socket in `socket_dir`.
/// The config is the default until the caller loads one with `load_config`.
pub fn resolve(
    session_flag: Option<&str>,
    endpoint_flag: Option<&str>,
    env_session: Option<&str>,
    env_endpoint: Option<&str>,
    socket_dir: &Path,
) -> Resolved {
    let session = session_flag.or(env_session).unwrap_or("main").to_string();
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

pub fn parse_agent_state(text: &str) -> Result<AgentState, String> {
    match text {
        "needs_input" => Ok(AgentState::NeedsInput),
        "working" => Ok(AgentState::Working),
        "idle" => Ok(AgentState::Idle),
        "exited" => Ok(AgentState::Exited),
        other => Err(format!(
            "unknown agent state: {other} (expected needs_input, working, idle, exited)"
        )),
    }
}

/// Condition selected by `--idle`, `--match` or `--state`; none means process exit.
pub fn wait_condition(
    idle: Option<u64>,
    pattern: Option<&str>,
    state: Option<&str>,
) -> Result<WaitCondition, String> {
    let chosen = [idle.is_some(), pattern.is_some(), state.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    if chosen > 1 {
        return Err("use only one of --idle, --match, --state".to_string());
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
    if let Some(state) = state {
        return Ok(WaitCondition::State {
            state: parse_agent_state(state)?,
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
