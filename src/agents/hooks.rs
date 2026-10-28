use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{AgentReportArgs, Client};
use crate::protocol::AgentState;

/// Claude Code hook events that mpx listens to; each runs `<exe> hook claude`.
const CLAUDE_EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "Notification",
    "Stop",
    "SessionEnd",
];

const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

const OMP_EXTENSION: &str = r#"import { spawn } from "node:child_process";

const MPX = __MPX_EXE__;

function report(event, data) {
  if (!process.env.MPX_PANE || !process.env.MPX_ENDPOINT) return;
  try {
    const child = spawn(MPX, ["hook", "omp", event], { stdio: ["pipe", "ignore", "ignore"], detached: true });
    child.on("error", () => {});
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(data));
    child.unref();
  } catch {}
}

export default function mpxAgentHooks(pi) {
  pi.on("session_start", (_event, ctx) =>
    report("session_start", { session_id: ctx?.sessionManager?.getSessionId?.() ?? null }));
  pi.on("agent_start", () => report("agent_start", {}));
  pi.on("turn_start", () => report("turn_start", {}));
  pi.on("agent_end", () => report("agent_end", {}));
  pi.on("tool_approval_requested", (event) => report("tool_approval_requested", { detail: event?.toolName ?? null }));
  pi.on("tool_approval_resolved", () => report("tool_approval_resolved", {}));
  pi.on("session_shutdown", () => report("session_shutdown", {}));
}
"#;

/// Agent state and bookkeeping produced by one harness hook event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HookReport {
    pub state: Option<AgentState>,
    pub session_id: Option<String>,
    pub detail: Option<String>,
    pub clear: bool,
}

impl HookReport {
    fn args(&self, harness: &str) -> AgentReportArgs {
        AgentReportArgs {
            harness: Some(harness.to_string()),
            state: self.state,
            session_id: self.session_id.clone(),
            detail: self.detail.clone(),
            clear: self.clear,
        }
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Claude Code `--settings` JSON registering `<exe> hook claude` for every mpx-relevant event.
pub fn claude_settings_json(exe: &Path) -> String {
    let command = format!("{} hook claude", shell_quote(&exe.to_string_lossy()));
    let entry = json!([{ "hooks": [{ "type": "command", "command": command }] }]);
    let hooks: serde_json::Map<String, Value> = CLAUDE_EVENTS
        .iter()
        .map(|event| (event.to_string(), entry.clone()))
        .collect();
    json!({ "hooks": hooks }).to_string()
}

/// Maps a Claude Code hook payload (stdin JSON) to an agent report.
pub fn claude_hook_report(input: &str) -> Option<HookReport> {
    let v: Value = serde_json::from_str(input).ok()?;
    let session_id = str_field(&v, "session_id");
    let event = str_field(&v, "hook_event_name")?;
    let (state, clear, detail) = match event.as_str() {
        "SessionStart" | "Stop" => (Some(AgentState::Idle), false, None),
        "UserPromptSubmit" | "PreToolUse" => (Some(AgentState::Working), false, None),
        "Notification" => (
            Some(AgentState::NeedsInput),
            false,
            str_field(&v, "message"),
        ),
        "SessionEnd" => (None, true, None),
        _ => return None,
    };
    Some(HookReport {
        state,
        session_id,
        detail,
        clear,
    })
}

/// Maps an omp extension event (`mpx hook omp <event>`, JSON on stdin) to an agent report.
pub fn omp_hook_report(event: &str, input: &str) -> Option<HookReport> {
    let v: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    let session_id = str_field(&v, "session_id");
    let detail = str_field(&v, "detail");
    let (state, clear) = match event {
        // A run (agent_start..agent_end) spans many turns; only the run boundary means idle.
        "session_start" | "agent_end" => (Some(AgentState::Idle), false),
        "agent_start" | "turn_start" | "tool_approval_resolved" => (Some(AgentState::Working), false),
        "tool_approval_requested" => (Some(AgentState::NeedsInput), false),
        "session_shutdown" => (None, true),
        _ => return None,
    };
    Some(HookReport {
        state,
        session_id,
        detail,
        clear,
    })
}

/// Source of the omp extension that forwards lifecycle events to `exe hook omp <event>`.
pub fn omp_extension_source(exe: &Path) -> String {
    let exe_literal =
        serde_json::to_string(&exe.to_string_lossy()).unwrap_or_else(|_| "\"mpx\"".to_string());
    OMP_EXTENSION.replace("__MPX_EXE__", &exe_literal)
}

/// Writes the omp extension into `<runtime_dir>/hooks` (only when its content changed) and returns its path.
pub fn omp_hook_file(runtime_dir: &Path) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = runtime_dir.join("hooks");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("omp-agent.js");
    let source = omp_extension_source(&exe);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(source.as_str()) {
        std::fs::write(&path, source).ok()?;
    }
    Some(path)
}

/// Reports a hook event for `$MPX_PANE` on `$MPX_ENDPOINT`; silently does nothing outside mpx or on failure.
pub async fn send_hook_report(harness: &str, report: HookReport, key: Option<&str>) {
    let (Ok(pane), Ok(endpoint)) = (std::env::var("MPX_PANE"), std::env::var("MPX_ENDPOINT"))
    else {
        return;
    };
    let send = async {
        let mut client = Client::connect(&endpoint).await.ok()?;
        if let Some(key) = key {
            client.idempotency_key(key);
        }
        client.agent_report(&pane, report.args(harness)).await.ok()
    };
    let _ = tokio::time::timeout(REPORT_TIMEOUT, send).await;
}
