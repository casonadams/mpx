//! Finds the session id of an AI CLI running in a pane from the state files the tool keeps for
//! its live processes, so restore can resume that exact session instead of `--continue`.
use crate::config::IdSource;
use std::path::Path;

/// Session id from Claude Code's `~/.claude/sessions/<pid>.json`. Rejects a file that names a
/// different pid.
pub fn claude_session_id(json: &str, pid: u32) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.get("pid")?.as_u64()? != u64::from(pid) {
        return None;
    }
    let id = value.get("sessionId")?.as_str()?;
    (!id.is_empty()).then(|| id.to_string())
}

/// Session id from oh-my-pi's `~/.omp/agent/terminal-sessions/<tty>`: the second line is the
/// session file `<timestamp>_<id>.jsonl`.
pub fn omp_session_id(terminal_session: &str) -> Option<String> {
    let path = terminal_session.lines().nth(1)?.trim();
    let stem = Path::new(path).file_stem()?.to_str()?;
    let (_, id) = stem.rsplit_once('_')?;
    (!id.is_empty()).then(|| id.to_string())
}

/// Reads the state file of `source` under `home` for the pane process `pid` on terminal `tty`
/// (device name without `/dev/`, e.g. `ttys004`).
pub fn lookup(source: IdSource, home: &Path, pid: Option<u32>, tty: Option<&str>) -> Option<String> {
    match source {
        IdSource::Claude => {
            let pid = pid?;
            let file = home.join(".claude/sessions").join(format!("{pid}.json"));
            claude_session_id(&std::fs::read_to_string(file).ok()?, pid)
        }
        IdSource::Omp => {
            let tty = tty.filter(|t| !t.is_empty() && !t.contains('/'))?;
            let file = home.join(".omp/agent/terminal-sessions").join(tty);
            omp_session_id(&std::fs::read_to_string(file).ok()?)
        }
    }
}
