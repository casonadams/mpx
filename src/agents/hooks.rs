use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{AgentReportArgs, Client};
use crate::protocol::AgentState;

/// What a harness hook event means for its agent.
#[derive(Clone, Copy)]
enum Meaning {
    State(AgentState),
    /// A new prompt, its text under the key: working on it.
    Prompted(&'static str),
    /// The run ended, its last reply under the key: idle.
    Finished(&'static str),
    /// Waiting for the user, what for under the key.
    Waiting(&'static str),
    /// A tool ran: an edit of the file `path` finds, when the tool (named under `tool`) is one of
    /// `edits`.
    Tool {
        tool: &'static str,
        edits: &'static [&'static str],
        path: fn(&Value) -> Option<String>,
    },
    Ended,
}

/// Claude Code hook events that mpx listens to, each running `<exe> hook claude`.
const CLAUDE_HOOKS: [(&str, Meaning); 7] = [
    ("SessionStart", Meaning::State(AgentState::Idle)),
    ("UserPromptSubmit", Meaning::Prompted("prompt")),
    ("PreToolUse", Meaning::State(AgentState::Working)),
    ("PostToolUse", Meaning::Tool { tool: "tool_name", edits: &EDIT_TOOLS, path: tool_file }),
    ("Notification", Meaning::Waiting("message")),
    ("Stop", Meaning::Finished("last_assistant_message")),
    ("SessionEnd", Meaning::Ended),
];

/// omp extension events (`mpx hook omp <event>`). A run (agent_start..agent_end) spans many
/// turns; only the run boundary means idle.
const OMP_HOOKS: [(&str, Meaning); 9] = [
    ("session_start", Meaning::State(AgentState::Idle)),
    ("before_agent_start", Meaning::Prompted("prompt")),
    ("agent_start", Meaning::State(AgentState::Working)),
    ("turn_start", Meaning::State(AgentState::Working)),
    ("tool_approval_resolved", Meaning::State(AgentState::Working)),
    ("tool_approval_requested", Meaning::Waiting("detail")),
    ("tool_result", Meaning::Tool { tool: "tool", edits: &OMP_EDIT_TOOLS, path: omp_tool_path }),
    ("agent_end", Meaning::Finished("reply")),
    ("session_shutdown", Meaning::Ended),
];
/// Tools whose use `PostToolUse` reports: file edits are recorded, and file access can surface
/// another agent's recent edit of the same file.
const CLAUDE_FILE_TOOLS: &str = "Read|Edit|Write|MultiEdit|NotebookEdit";
const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
/// Longest task or reply text kept for other agents to read.
pub const FOCUS_CHARS: usize = 240;
/// Longest final reply kept: what `mpx_agent_prompt` hands back to the agent that asked.
const REPLY_CHARS: usize = 4000;

const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// The omp extension, with `__MPX_EXE__` standing for the mpx binary.
const OMP_EXTENSION: &str = include_str!("omp_extension.js");

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
    cut(&text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// `text` cut to `max` characters, ending in `…` when it was longer.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
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
    let hooks: serde_json::Map<String, Value> = CLAUDE_HOOKS
        .iter()
        .map(|(event, _)| {
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
    report_for(&v, &str_field(&v, "hook_event_name")?, &CLAUDE_HOOKS)
}

/// The report `event` makes with payload `v`, by what `hooks` say it means.
fn report_for(v: &Value, event: &str, hooks: &[(&str, Meaning)]) -> Option<HookReport> {
    let (_, meaning) = hooks.iter().find(|(name, _)| *name == event)?;
    let p = Payload(v);
    match *meaning {
        Meaning::State(state) => p.in_state(state),
        Meaning::Prompted(key) => p.prompted(key),
        Meaning::Finished(key) => p.finished(key),
        Meaning::Waiting(key) => p.waiting(key),
        Meaning::Tool { tool, edits, path } => p.edited(str_field(v, tool), edits, path(v)),
        Meaning::Ended => p.ended(),
    }
}

/// Builds the reports of one hook payload; its session id goes with every one.
struct Payload<'a>(&'a Value);

impl Payload<'_> {
    fn report(&self, state: Option<AgentState>) -> HookReport {
        HookReport {
            state,
            session_id: str_field(self.0, "session_id"),
            ..HookReport::default()
        }
    }

    fn in_state(&self, state: AgentState) -> Option<HookReport> {
        Some(self.report(Some(state)))
    }

    /// The agent took a prompt (its text under `key`): working on it.
    fn prompted(&self, key: &str) -> Option<HookReport> {
        let task = self.focus(key);
        Some(HookReport {
            task,
            ..self.report(Some(AgentState::Working))
        })
    }

    /// The agent finished its run (its last reply under `key`, kept with its lines): idle.
    fn finished(&self, key: &str) -> Option<HookReport> {
        let reply = str_field(self.0, key).map(|t| cut(t.trim(), REPLY_CHARS));
        Some(HookReport {
            reply,
            ..self.report(Some(AgentState::Idle))
        })
    }

    /// The agent waits for the user (what for under `key`).
    fn waiting(&self, key: &str) -> Option<HookReport> {
        let detail = str_field(self.0, key);
        Some(HookReport {
            detail,
            ..self.report(Some(AgentState::NeedsInput))
        })
    }

    /// `tool` ran on `path`: an edit worth recording when `tool` is one of `edit_tools`.
    fn edited(
        &self,
        tool: Option<String>,
        edit_tools: &[&str],
        path: Option<String>,
    ) -> Option<HookReport> {
        if !edit_tools.contains(&tool?.as_str()) {
            return None;
        }
        Some(HookReport {
            edited: Some(path?),
            ..self.report(None)
        })
    }

    fn ended(&self) -> Option<HookReport> {
        Some(HookReport {
            clear: true,
            ..self.report(None)
        })
    }

    fn focus(&self, key: &str) -> Option<String> {
        str_field(self.0, key).map(|t| clip(&t, FOCUS_CHARS))
    }
}

/// omp tools whose results mean the agent changed a file.
const OMP_EDIT_TOOLS: [&str; 2] = ["write", "edit"];

/// Maps an omp extension event (`mpx hook omp <event>`, JSON on stdin) to an agent report.
pub fn omp_hook_report(event: &str, input: &str) -> Option<HookReport> {
    let v: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    report_for(&v, event, &OMP_HOOKS)
}

/// The file an omp tool worked on: the extension sends it resolved. Never one of omp's virtual
/// paths (`xd://...`, through which it runs MCP tools), which an older extension could send.
fn omp_tool_path(v: &Value) -> Option<String> {
    str_field(v, "path").filter(|path| !path.contains("://"))
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
    write_if_changed(&path, &omp_extension_source(&exe))?;
    Some(path)
}

/// Writes an omp plugin (`--plugin-dir`) into `<runtime_dir>/hooks/omp-plugin` that gives the agent
/// mpx's MCP tools, and returns its directory. omp runs a plugin's MCP server only from a bare
/// command name or a file inside the plugin, so the plugin holds a small script that runs `exe`.
pub fn omp_plugin_dir(runtime_dir: &Path, exe: &Path) -> Option<PathBuf> {
    let dir = runtime_dir.join("hooks").join("omp-plugin");
    std::fs::create_dir_all(&dir).ok()?;
    let plugin = json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": "mpx",
    });
    let mcp = json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": { "mpx": { "type": "stdio", "command": "./mpx", "args": ["mcp"] } },
    });
    write_if_changed(&dir.join("plugin.json"), &plugin.to_string())?;
    write_if_changed(&dir.join("mcp.json"), &mcp.to_string())?;
    let script = dir.join("mpx");
    let run = format!("#!/bin/sh\nexec {} \"$@\"\n", shell_quote(&exe.to_string_lossy()));
    write_if_changed(&script, &run)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).ok()?;
    }
    Some(dir)
}

fn write_if_changed(path: &Path, content: &str) -> Option<()> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(content) {
        std::fs::write(path, content).ok()?;
    }
    Some(())
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
