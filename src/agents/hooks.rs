use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{AgentReportArgs, Client};
use crate::protocol::AgentState;

/// Claude Code hook events that mpx listens to; each runs `<exe> hook claude`.
const CLAUDE_EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "SessionEnd",
];
/// Tools whose use `PostToolUse` reports: file edits are recorded, and file access can surface
/// another agent's recent edit of the same file.
const CLAUDE_FILE_TOOLS: &str = "Read|Edit|Write|MultiEdit|NotebookEdit";
const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
/// Longest task or reply text kept for other agents to read.
const FOCUS_CHARS: usize = 240;

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
    pub task: Option<String>,
    pub reply: Option<String>,
    pub edited: Option<String>,
}

impl HookReport {
    fn args(&self, harness: &str) -> AgentReportArgs {
        AgentReportArgs {
            harness: Some(harness.to_string()),
            state: self.state,
            session_id: self.session_id.clone(),
            detail: self.detail.clone(),
            clear: self.clear,
            task: self.task.clone(),
            reply: self.reply.clone(),
            edited: self.edited.clone(),
        }
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `text` on one line with whitespace runs collapsed, cut to `max` characters with `…`.
pub fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// The file a Claude Code tool call works on: `file_path`, or `notebook_path` for notebooks.
pub fn tool_file(v: &Value) -> Option<String> {
    let input = v.get("tool_input")?;
    str_field(input, "file_path").or_else(|| str_field(input, "notebook_path"))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Claude Code `--settings` JSON registering `<exe> hook claude` for every mpx-relevant event.
pub fn claude_settings_json(exe: &Path) -> String {
    let command = format!("{} hook claude", shell_quote(&exe.to_string_lossy()));
    let hook = json!([{ "type": "command", "command": command }]);
    let hooks: serde_json::Map<String, Value> = CLAUDE_EVENTS
        .iter()
        .map(|event| {
            let entry = match *event {
                "PostToolUse" => json!({ "matcher": CLAUDE_FILE_TOOLS, "hooks": hook }),
                _ => json!({ "hooks": hook }),
            };
            (event.to_string(), json!([entry]))
        })
        .collect();
    json!({ "hooks": hooks }).to_string()
}

/// Maps a Claude Code hook payload (stdin JSON) to an agent report.
pub fn claude_hook_report(input: &str) -> Option<HookReport> {
    let v: Value = serde_json::from_str(input).ok()?;
    let session_id = str_field(&v, "session_id");
    let event = str_field(&v, "hook_event_name")?;
    let focus = |key: &str| str_field(&v, key).map(|t| clip(&t, FOCUS_CHARS));
    let mut report = HookReport {
        session_id,
        ..HookReport::default()
    };
    match event.as_str() {
        "SessionStart" => report.state = Some(AgentState::Idle),
        "Stop" => {
            report.state = Some(AgentState::Idle);
            report.reply = focus("last_assistant_message");
        }
        "UserPromptSubmit" => {
            report.state = Some(AgentState::Working);
            report.task = focus("prompt");
        }
        "PreToolUse" => report.state = Some(AgentState::Working),
        "PostToolUse" => {
            let tool = str_field(&v, "tool_name").unwrap_or_default();
            if !EDIT_TOOLS.contains(&tool.as_str()) {
                return None;
            }
            report.edited = Some(tool_file(&v)?);
        }
        "Notification" => {
            report.state = Some(AgentState::NeedsInput);
            report.detail = str_field(&v, "message");
        }
        "SessionEnd" => report.clear = true,
        _ => return None,
    }
    Some(report)
}

/// Maps an omp extension event (`mpx hook omp <event>`, JSON on stdin) to an agent report.
pub fn omp_hook_report(event: &str, input: &str) -> Option<HookReport> {
    let v: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    let session_id = str_field(&v, "session_id");
    let detail = str_field(&v, "detail");
    let (state, clear) = match event {
        // A run (agent_start..agent_end) spans many turns; only the run boundary means idle.
        "session_start" | "agent_end" => (Some(AgentState::Idle), false),
        "agent_start" | "turn_start" | "tool_approval_resolved" => {
            (Some(AgentState::Working), false)
        }
        "tool_approval_requested" => (Some(AgentState::NeedsInput), false),
        "session_shutdown" => (None, true),
        _ => return None,
    };
    Some(HookReport {
        state,
        session_id,
        detail,
        clear,
        ..HookReport::default()
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
