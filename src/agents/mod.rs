//! AI agent harness registry (claude, omp, codex, ...), resumable session discovery, launch specs
//! and live-agent listing across mpx sessions.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::client::Client;
use crate::protocol::{AgentState, NodeInfo};

mod harness;
mod hooks;
mod sessions;

pub use harness::{LaunchEnv, detect, launch, launch_with, registry};
pub use hooks::{
    HookReport, claude_hook_report, claude_settings_json, omp_extension_source, omp_hook_file,
    omp_hook_report, send_hook_report,
};
pub use sessions::{
    SessionMeta, Visit, lookup_rho_cwd, parse_claude_meta, parse_codex_meta, parse_omp_meta,
    parse_preview, parse_rho_meta, scan_resumable, transcript_preview, visit_entry,
};

const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
const SHORT_ID_CHARS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStore {
    /// `~/.claude/projects/<cwd-slug>/<uuid>.jsonl`
    Claude,
    /// `~/.omp/agent/sessions/<cwd-slug>/<ts>_<id>.jsonl`
    Omp,
    /// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
    Codex,
    /// `~/.config/rho/sessions/<id>.jsonl`
    Rho,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Harness {
    pub name: String,
    pub detect: Vec<String>,
    pub command: Vec<String>,
    pub resume: Vec<String>,
    pub session_id_args: Vec<String>,
    pub store: Option<SessionStore>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Claude Code agent teams: tmux compat + CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS.
    pub teams: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    /// Suggested pane/window name.
    pub name: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub tmux_compat: bool,
    /// Session id pre-assigned or resumed, when known.
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumableSession {
    pub harness: String,
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub modified_ms: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LiveAgent {
    pub session: String,
    pub endpoint: String,
    pub node: NodeInfo,
}

/// Display label for an agent state.
pub fn state_label(state: AgentState) -> &'static str {
    match state {
        AgentState::NeedsInput => "needs_input",
        AgentState::Working => "working",
        AgentState::Idle => "idle",
        AgentState::Exited => "exited",
    }
}

/// Harness with exactly this name.
pub fn find_harness<'a>(registry: &'a [Harness], name: &str) -> Option<&'a Harness> {
    registry.iter().find(|h| h.name == name)
}

/// The single resumable session whose id starts with `prefix`; errors when none or several match.
pub fn find_unique_by_prefix(
    sessions: Vec<ResumableSession>,
    prefix: &str,
) -> Result<ResumableSession, String> {
    let mut matches: Vec<ResumableSession> = sessions
        .into_iter()
        .filter(|s| s.id.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => Err(format!("no stored session matches '{prefix}'")),
        1 => Ok(matches.remove(0)),
        n => Err(format!(
            "{n} sessions match '{prefix}'; use a longer prefix"
        )),
    }
}

/// Pane name for a launched harness: the spec name plus a short session id (or random suffix).
pub fn default_node_name(spec: &LaunchSpec) -> String {
    let short: String = match &spec.session_id {
        Some(id) => id.chars().take(SHORT_ID_CHARS).collect(),
        None => Uuid::new_v4().simple().to_string()[..SHORT_ID_CHARS].to_string(),
    };
    format!("{}-{}", spec.name, short)
}

/// Agent report for a hook payload: `claude` reads the event from the payload, `omp` gets it as an argument.
pub fn hook_report(harness: &str, event: Option<&str>, input: &str) -> Option<HookReport> {
    match harness {
        "claude" => claude_hook_report(input),
        "omp" => omp_hook_report(event?, input),
        _ => None,
    }
}

pub fn live_json(agent: &LiveAgent) -> Value {
    json!({
        "session": agent.session,
        "endpoint": agent.endpoint,
        "node": agent.node,
    })
}

pub fn resumable_json(session: &ResumableSession) -> Value {
    json!({
        "harness": session.harness,
        "id": session.id,
        "title": session.title,
        "cwd": session.cwd,
        "modified_ms": session.modified_ms,
        "path": session.path.to_string_lossy(),
    })
}

/// `{live, resumable}`: agents running in the sessions of `socket_dir` and the most recent stored sessions.
pub async fn inventory_json(registry: &[Harness], limit: usize, socket_dir: &Path) -> Value {
    let live: Vec<Value> = list_live(socket_dir).await.iter().map(live_json).collect();
    let resumable: Vec<Value> = scan_resumable(registry, None, limit)
        .iter()
        .map(resumable_json)
        .collect();
    json!({ "live": live, "resumable": resumable })
}

/// Nodes with agent info from every reachable mpx session whose socket is in `socket_dir`.
pub async fn list_live(socket_dir: &Path) -> Vec<LiveAgent> {
    let probes = crate::ipc::list_sessions(socket_dir)
        .into_iter()
        .map(|ep| async move {
            let Ok(Ok(mut client)) =
                tokio::time::timeout(PROBE_TIMEOUT, Client::connect(&ep.endpoint)).await
            else {
                return Vec::new();
            };
            let Ok(Ok(nodes)) = tokio::time::timeout(PROBE_TIMEOUT, client.list()).await else {
                return Vec::new();
            };
            nodes
                .into_iter()
                .filter(|n| n.agent.is_some())
                .map(|node| LiveAgent {
                    session: ep.name.clone(),
                    endpoint: ep.endpoint.clone(),
                    node,
                })
                .collect::<Vec<_>>()
        });
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect()
}
