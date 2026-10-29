//! Agent awareness: which other agents work in the same repository, what they are working on, and
//! which files they edited. Hooks feed it to an agent sparingly (once per change); `mpx_peers`
//! answers on demand.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{LiveAgent, list_live, state_label, title_of};
use crate::protocol::{AgentState, EDIT_WINDOW_MS};

/// The agent asking: its pane, the endpoint of its session and its working directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Me {
    pub endpoint: String,
    pub pane: String,
    pub cwd: PathBuf,
}

/// The nearest ancestor of `dir` (itself included) holding `.git`, else `dir`; symlinks resolved,
/// since a pane's reported directory and an agent's own view of it can differ by a symlink.
pub fn repo_root(dir: &Path) -> PathBuf {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(&dir)
        .to_path_buf()
}

/// `session:pane`, the name other agents and `mpx_agent_prompt` use for an agent.
pub fn agent_ref(agent: &LiveAgent) -> String {
    format!("{}:{}", agent.session, agent.node.name)
}

fn is_me(agent: &LiveAgent, me: &Me) -> bool {
    agent.endpoint == me.endpoint && agent.node.name == me.pane
}

/// Running agents other than `me` whose working directory is in `me`'s repository.
pub fn peers(live: Vec<LiveAgent>, me: &Me) -> Vec<LiveAgent> {
    let root = repo_root(&me.cwd);
    live.into_iter()
        .filter(|a| !is_me(a, me) && a.node.is_running())
        .filter(|a| {
            a.node
                .cwd
                .as_deref()
                .is_some_and(|cwd| repo_root(Path::new(cwd)) == root)
        })
        .collect()
}

fn ago(now_ms: u64, at_ms: u64) -> String {
    let mins = now_ms.saturating_sub(at_ms) / 60_000;
    match mins {
        0 => "just now".to_string(),
        1 => "1 minute ago".to_string(),
        n => format!("{n} minutes ago"),
    }
}

/// One line per peer: reference, harness, state, title and current task.
fn peer_line(agent: &LiveAgent) -> String {
    let info = agent.node.agent.as_ref();
    let harness = info.map_or("agent", |a| a.harness.as_str());
    let state = info.map_or("idle", |a| state_label(a.state));
    let mut line = format!("- {} ({harness}, {state})", agent_ref(agent));
    if let Some(title) = title_of(&agent.node) {
        line.push_str(&format!(" \"{title}\""));
    }
    if let Some(task) = info.and_then(|a| a.focus.task.as_deref()) {
        line.push_str(&format!("; task: {task}"));
    }
    line
}

/// What an agent is told when the set of agents in its repository changes.
pub fn roster_text(peers: &[LiveAgent]) -> String {
    if peers.is_empty() {
        return "mpx: no other agents are working in this repository now.".to_string();
    }
    let mut text = String::from(
        "mpx: other agents are working in this repository and may change files you use:\n",
    );
    for agent in peers {
        text.push_str(&peer_line(agent));
        text.push('\n');
    }
    text.push_str(
        "Use mpx_peers to see what they are doing, and mpx_agent_prompt with an agent's \
         session:pane name to ask one how to avoid conflicting work.",
    );
    text
}

/// Peers that edited `path` within `EDIT_WINDOW_MS` of `now_ms`, with the time of that edit.
pub fn editors_of<'a>(peers: &'a [LiveAgent], path: &str, now_ms: u64) -> Vec<(&'a LiveAgent, u64)> {
    peers
        .iter()
        .filter_map(|a| {
            let edits = &a.node.agent.as_ref()?.focus.edits;
            let edit = edits.iter().rev().find(|e| e.path == path)?;
            (now_ms.saturating_sub(edit.at_ms) < EDIT_WINDOW_MS).then_some((a, edit.at_ms))
        })
        .collect()
}

/// What an agent is told when it uses a file other agents edited recently.
pub fn file_notice(path: &str, editors: &[(&LiveAgent, u64)], now_ms: u64) -> String {
    let mut text = String::new();
    for (agent, at_ms) in editors {
        let task = agent
            .node
            .agent
            .as_ref()
            .and_then(|a| a.focus.task.as_deref())
            .map(|t| format!(" while working on: {t}"))
            .unwrap_or_default();
        text.push_str(&format!(
            "mpx: {path} was edited {} by agent {}{task}\n",
            ago(now_ms, *at_ms),
            agent_ref(agent)
        ));
    }
    text.push_str("If your work overlaps, coordinate with it using mpx_agent_prompt.");
    text
}

/// What an agent has already been told, so nothing is repeated.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    /// Peer references in the last roster it was given.
    pub peers: Vec<String>,
    /// `path|peer` pairs it was already told about.
    pub noticed: BTreeSet<String>,
}

fn seen_file(dir: &Path, id: &str) -> PathBuf {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    dir.join(format!("{safe}.json"))
}

impl Seen {
    /// What agent `id` was told, from `dir`; nothing when absent or unreadable.
    pub fn load(dir: &Path, id: &str) -> Self {
        std::fs::read_to_string(seen_file(dir, id))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path, id: &str) {
        if std::fs::create_dir_all(dir).is_ok()
            && let Ok(text) = serde_json::to_string(self)
        {
            let _ = std::fs::write(seen_file(dir, id), text);
        }
    }

    pub fn forget(dir: &Path, id: &str) {
        let _ = std::fs::remove_file(seen_file(dir, id));
    }

    /// The roster to send when the peers differ from the last roster sent (an empty set counts
    /// only after a non-empty one), remembering them.
    pub fn roster_update(&mut self, peers: &[LiveAgent]) -> Option<String> {
        let mut refs: Vec<String> = peers.iter().map(agent_ref).collect();
        refs.sort();
        if refs == self.peers {
            return None;
        }
        self.peers = refs;
        Some(roster_text(peers))
    }

    /// A notice about recent edits of `path` by peers not yet reported for it, remembering them.
    pub fn file_update(&mut self, path: &str, peers: &[LiveAgent], now_ms: u64) -> Option<String> {
        let fresh: Vec<(&LiveAgent, u64)> = editors_of(peers, path, now_ms)
            .into_iter()
            .filter(|(a, _)| self.noticed.insert(format!("{path}|{}", agent_ref(a))))
            .collect();
        (!fresh.is_empty()).then(|| file_notice(path, &fresh, now_ms))
    }
}

/// When a hook event warrants telling the agent something.
#[derive(Debug, Clone, PartialEq)]
pub enum Moment {
    /// Session start or a new prompt: the roster, if it changed.
    Roster,
    /// A file tool was used on this path: other agents' recent edits of it.
    File(String),
}

/// The moment a Claude Code hook payload represents, if any.
pub fn claude_moment(v: &Value) -> Option<Moment> {
    match v.get("hook_event_name")?.as_str()? {
        "SessionStart" | "UserPromptSubmit" => Some(Moment::Roster),
        "PostToolUse" => super::hooks::tool_file(v).map(Moment::File),
        _ => None,
    }
}

/// The moment an omp extension event represents, if any: a new prompt, or a file tool's result
/// (its absolute path, as the extension sends it).
pub fn omp_moment(event: &str, v: &Value) -> Option<Moment> {
    match event {
        "before_agent_start" => Some(Moment::Roster),
        "tool_result" => Some(Moment::File(v.get("path")?.as_str()?.to_string())),
        _ => None,
    }
}

/// `path` with symlinks resolved, so agents that name one file differently (`/var/...` and
/// `/private/var/...` on macOS) still match; as given when it cannot be resolved.
pub fn real_path(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(|_| path.to_string(), |p| p.to_string_lossy().into_owned())
}

/// What a harness hook event means for awareness.
#[derive(Debug, Clone, PartialEq)]
pub struct HookEvent {
    pub name: String,
    pub moment: Option<Moment>,
    /// The agent's session ended: what it was told can be forgotten.
    pub ends: bool,
    pub session_id: Option<String>,
    pub cwd: Option<PathBuf>,
}

impl HookEvent {
    /// A Claude Code hook payload, which names its own event.
    pub fn claude(v: &Value) -> Option<Self> {
        let name = v.get("hook_event_name")?.as_str()?.to_string();
        Some(Self::with(v, claude_moment(v), name == "SessionEnd", name))
    }

    /// An omp extension event, named on the command line.
    pub fn omp(event: &str, v: &Value) -> Self {
        Self::with(v, omp_moment(event, v), event == "session_shutdown", event.to_string())
    }

    fn with(v: &Value, moment: Option<Moment>, ends: bool, name: String) -> Self {
        let field = |key: &str| v.get(key).and_then(Value::as_str);
        Self {
            name,
            moment,
            ends,
            session_id: field("session_id").map(str::to_string),
            cwd: field("cwd").map(PathBuf::from),
        }
    }
}

/// Hook stdout handing `text` to the model as context for `event`.
pub fn context_json(event: &str, text: &str) -> String {
    json!({
        "hookSpecificOutput": { "hookEventName": event, "additionalContext": text }
    })
    .to_string()
}

/// Where a hook runs: the agent's pane and session, and the directory holding every session and
/// per-agent memory.
pub struct HookSite<'a> {
    pub endpoint: &'a str,
    pub pane: &'a str,
    pub socket_dir: &'a Path,
}

/// What to tell the agent behind `event` about other agents, remembering it was told. `None` when
/// there is nothing new to say.
pub async fn context_text(event: &HookEvent, site: &HookSite<'_>, now_ms: u64) -> Option<String> {
    let id = event.session_id.as_deref().unwrap_or(site.pane);
    let dir = site.socket_dir.join("aware");
    if event.ends {
        Seen::forget(&dir, id);
        return None;
    }
    let moment = event.moment.as_ref()?;
    let me = Me {
        endpoint: site.endpoint.to_string(),
        pane: site.pane.to_string(),
        cwd: event.cwd.clone()?,
    };
    let peers = peers(list_live(site.socket_dir).await, &me);
    let mut seen = Seen::load(&dir, id);
    let text = match moment {
        Moment::Roster => seen.roster_update(&peers),
        Moment::File(path) => seen.file_update(&real_path(path), &peers, now_ms),
    }?;
    seen.save(&dir, id);
    Some(text)
}

/// `mpx_peers` result: agents in `me`'s repository (every agent with `all`), with what each is
/// working on and the files it edited recently.
pub fn peers_json(live: Vec<LiveAgent>, me: &Me, all: bool, now_ms: u64) -> Value {
    // The caller's own name, so it can tell itself apart when another agent mentions it.
    let you = live.iter().find(|a| is_me(a, me)).map(agent_ref);
    let agents: Vec<LiveAgent> = if all {
        live.into_iter().filter(|a| !is_me(a, me)).collect()
    } else {
        peers(live, me)
    };
    let rows: Vec<Value> = agents
        .iter()
        .map(|a| {
            let info = a.node.agent.as_ref();
            let edits: Vec<Value> = info
                .map(|i| i.focus.edits.iter().rev())
                .into_iter()
                .flatten()
                .filter(|e| now_ms.saturating_sub(e.at_ms) < EDIT_WINDOW_MS)
                .map(|e| json!({ "path": e.path, "ago": ago(now_ms, e.at_ms) }))
                .collect();
            json!({
                "agent": agent_ref(a),
                "harness": info.map(|i| i.harness.clone()),
                "state": state_label(info.map_or(AgentState::Idle, |i| i.state)),
                "title": title_of(&a.node),
                "task": info.and_then(|i| i.focus.task.clone()),
                "last_reply": info.and_then(|i| i.focus.reply.clone()),
                "cwd": a.node.cwd,
                "recent_edits": edits,
            })
        })
        .collect();
    json!({ "you": you, "repository": repo_root(&me.cwd), "agents": rows })
}
