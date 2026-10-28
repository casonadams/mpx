use serde_json::{Map, Value, json};

use crate::cli::{parse_agent_state, parse_env_pairs};
use crate::client::PipeReadArgs;
use crate::layout::Direction;
use crate::protocol::{SpawnOpts, WaitCondition};

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Str,
    Int,
    Bool,
    StrList,
    Enum(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct Prop {
    pub name: &'static str,
    pub kind: Kind,
    pub desc: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Tool {
    pub name: &'static str,
    pub desc: &'static str,
    pub props: &'static [Prop],
    pub required: &'static [&'static str],
}

const fn prop(name: &'static str, kind: Kind, desc: &'static str) -> Prop {
    Prop { name, kind, desc }
}

const SESSION_NOTE: &str = "Target node or pane name (or %id).";
const KEY_NOTE: &str =
    "Idempotency key: a repeated key returns the first result instead of running again.";
const KEY_PROP: Prop = prop("idempotency_key", Kind::Str, KEY_NOTE);

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "mpx_run",
        desc: "Start a command in a new stream node (pipe or PTY).",
        props: &[
            prop("command", Kind::Str, "Program to run"),
            prop("args", Kind::StrList, "Arguments"),
            prop(
                "name",
                Kind::Str,
                "Node name (defaults to the program name)",
            ),
            prop("pty", Kind::Bool, "Allocate an interactive PTY"),
            prop("cwd", Kind::Str, "Working directory"),
            prop("env", Kind::StrList, "Extra environment as K=V"),
            prop("remain", Kind::Bool, "Keep the pane after exit"),
            KEY_PROP,
        ],
        required: &["command"],
    },
    Tool {
        name: "mpx_list",
        desc: "List nodes in the session with PIDs, types, statuses and agent state.",
        props: &[],
        required: &[],
    },
    Tool {
        name: "mpx_status",
        desc: "Status, PID, exit code and agent state of one node.",
        props: &[prop("name", Kind::Str, SESSION_NOTE)],
        required: &["name"],
    },
    Tool {
        name: "mpx_read",
        desc: "Read output of a node. Returns {text, next}; pass next as since to read only new output.",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop("tail", Kind::Int, "Only the last N lines of scrollback"),
            prop("plain", Kind::Bool, "Strip escape sequences"),
            prop(
                "since",
                Kind::Int,
                "Byte offset returned as next by a previous call",
            ),
        ],
        required: &["name"],
    },
    Tool {
        name: "mpx_screen",
        desc: "Snapshot the rendered terminal screen of a node.",
        props: &[prop("name", Kind::Str, SESSION_NOTE)],
        required: &["name"],
    },
    Tool {
        name: "mpx_send",
        desc: "Write raw bytes to a node's stdin (a newline is appended when missing).",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop("data", Kind::Str, "Raw text to write"),
            KEY_PROP,
        ],
        required: &["name", "data"],
    },
    Tool {
        name: "mpx_send_keys",
        desc: "Send tmux-style key names (Enter, C-c, S-Tab, F5) or literal text to a pane.",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop(
                "keys",
                Kind::StrList,
                "Key names, or text when literal is true",
            ),
            prop("literal", Kind::Bool, "Send keys as literal text"),
            KEY_PROP,
        ],
        required: &["name", "keys"],
    },
    Tool {
        name: "mpx_paste",
        desc: "Paste text into a pane (bracketed when the program enabled it), optionally pressing Enter.",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop("text", Kind::Str, "Text to paste"),
            prop("submit", Kind::Bool, "Press Enter after pasting"),
            KEY_PROP,
        ],
        required: &["name", "text"],
    },
    Tool {
        name: "mpx_wait",
        desc: "Wait for a node: exit (default), idle output, screen match, or agent state.",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop(
                "condition",
                Kind::Enum(&["exit", "idle", "match", "state"]),
                "What to wait for",
            ),
            prop(
                "quiet_ms",
                Kind::Int,
                "Quiet period for condition idle (default 2000)",
            ),
            prop("pattern", Kind::Str, "Regex for condition match"),
            prop(
                "state",
                Kind::Enum(&["needs_input", "working", "idle", "exited"]),
                "Agent state for condition state",
            ),
            prop("timeout", Kind::Int, "Timeout in seconds"),
        ],
        required: &["name"],
    },
    Tool {
        name: "mpx_kill",
        desc: "Terminate a stream node.",
        props: &[prop("name", Kind::Str, SESSION_NOTE), KEY_PROP],
        required: &["name"],
    },
    Tool {
        name: "mpx_split",
        desc: "Split a pane and run a command in the new pane.",
        props: &[
            prop("command", Kind::Str, "Program to run"),
            prop("args", Kind::StrList, "Arguments"),
            prop("name", Kind::Str, "Node name"),
            prop(
                "direction",
                Kind::Enum(&["h", "v"]),
                "h: side by side, v: stacked",
            ),
            prop("percent", Kind::Int, "Size of the new pane as a percentage"),
            prop("target", Kind::Str, "Pane to split (default: focused pane)"),
            prop("pty", Kind::Bool, "Allocate an interactive PTY"),
            prop("focus", Kind::Bool, "Focus the new pane (default false)"),
            prop("cwd", Kind::Str, "Working directory"),
            KEY_PROP,
        ],
        required: &["command"],
    },
    Tool {
        name: "mpx_new_window",
        desc: "Open a new window running a command.",
        props: &[
            prop("command", Kind::Str, "Program to run"),
            prop("args", Kind::StrList, "Arguments"),
            prop("name", Kind::Str, "Window and node name"),
            prop("pty", Kind::Bool, "Allocate an interactive PTY"),
            prop("focus", Kind::Bool, "Focus the new window (default false)"),
            prop("cwd", Kind::Str, "Working directory"),
            KEY_PROP,
        ],
        required: &["command"],
    },
    Tool {
        name: "mpx_focus",
        desc: "Move the user's focus to a pane.",
        props: &[prop("name", Kind::Str, SESSION_NOTE)],
        required: &["name"],
    },
    Tool {
        name: "mpx_pipe_create",
        desc: "Create a virtual pipe for inter-agent messages.",
        props: &[
            prop("name", Kind::Str, "Pipe name"),
            prop("capacity", Kind::Int, "Maximum capacity in bytes"),
        ],
        required: &["name"],
    },
    Tool {
        name: "mpx_pipe_write",
        desc: "Append a message to a virtual pipe (created on first use). Returns its sequence number.",
        props: &[
            prop("name", Kind::Str, "Pipe name"),
            prop("data", Kind::Str, "Message"),
            KEY_PROP,
        ],
        required: &["name", "data"],
    },
    Tool {
        name: "mpx_pipe_read",
        desc: "Read messages from a virtual pipe (created on first use), optionally through a named consumer cursor.",
        props: &[
            prop("name", Kind::Str, "Pipe name"),
            prop(
                "since",
                Kind::Int,
                "Only messages with a sequence number after this",
            ),
            prop("consumer", Kind::Str, "Daemon-side cursor name"),
            prop(
                "wait_ms",
                Kind::Int,
                "Block up to this many milliseconds for a message",
            ),
            prop("limit", Kind::Int, "Maximum number of messages"),
        ],
        required: &["name"],
    },
    Tool {
        name: "mpx_pipe_list",
        desc: "List virtual pipes in the session.",
        props: &[],
        required: &[],
    },
    Tool {
        name: "mpx_restore",
        desc: "Recreate the session's windows, panes and agents from its last snapshot. Errors when the session already has panes or no snapshot exists.",
        props: &[],
        required: &[],
    },
    Tool {
        name: "mpx_sessions",
        desc: "List reachable mpx sessions with window and pane counts.",
        props: &[],
        required: &[],
    },
    Tool {
        name: "mpx_agents",
        desc: "List live agents (with state) across sessions and resumable stored sessions.",
        props: &[],
        required: &[],
    },
    Tool {
        name: "mpx_agent_launch",
        desc: "Launch an agent harness (claude, omp, codex, ...) in a new window or split, optionally resuming a stored session.",
        props: &[
            prop("harness", Kind::Str, "Harness name"),
            prop("resume", Kind::Str, "Session id to resume"),
            prop("cwd", Kind::Str, "Working directory"),
            prop(
                "split",
                Kind::Bool,
                "Split the focused pane instead of a new window",
            ),
            prop("teams", Kind::Bool, "Enable agent teams"),
            prop("name", Kind::Str, "Node name"),
            KEY_PROP,
        ],
        required: &["harness"],
    },
    Tool {
        name: "mpx_agent_prompt",
        desc: "Send a prompt to an agent pane, wait for it to go idle or need input, and return the screen.",
        props: &[
            prop("name", Kind::Str, SESSION_NOTE),
            prop("text", Kind::Str, "Prompt text"),
            prop(
                "wait",
                Kind::Bool,
                "Wait for the agent to finish (default true)",
            ),
            prop("timeout", Kind::Int, "Seconds to wait (default 120)"),
            KEY_PROP,
        ],
        required: &["name", "text"],
    },
];

fn prop_schema(p: &Prop) -> Value {
    let (kind, extra): (&str, Value) = match p.kind {
        Kind::Str => ("string", Value::Null),
        Kind::Int => ("integer", Value::Null),
        Kind::Bool => ("boolean", Value::Null),
        Kind::StrList => ("array", json!({ "items": { "type": "string" } })),
        Kind::Enum(values) => ("string", json!({ "enum": values })),
    };
    let mut schema = json!({ "type": kind, "description": p.desc });
    if let (Some(obj), Value::Object(extra)) = (schema.as_object_mut(), extra) {
        obj.extend(extra);
    }
    schema
}

fn tool_schema(t: &Tool) -> Value {
    let props: Map<String, Value> = t
        .props
        .iter()
        .map(|p| (p.name.to_string(), prop_schema(p)))
        .collect();
    json!({
        "name": t.name,
        "description": t.desc,
        "inputSchema": { "type": "object", "properties": props, "required": t.required },
    })
}

/// `tools/list` payload.
pub fn tool_list() -> Value {
    Value::Array(TOOLS.iter().map(tool_schema).collect())
}

/// Typed operation derived from one `tools/call`. `key` is applied to the call's mutating request.
#[derive(Debug)]
pub enum Call {
    Run {
        spawn: SpawnOpts,
        key: Option<String>,
    },
    List,
    Status(String),
    Read {
        name: String,
        tail: Option<usize>,
        plain: bool,
        since: Option<u64>,
    },
    Screen(String),
    Send {
        name: String,
        data: String,
        key: Option<String>,
    },
    SendKeys {
        name: String,
        keys: Vec<String>,
        literal: bool,
        key: Option<String>,
    },
    Paste {
        name: String,
        text: String,
        submit: bool,
        key: Option<String>,
    },
    Wait {
        name: String,
        condition: WaitCondition,
        timeout_ms: Option<u64>,
    },
    Kill {
        name: String,
        key: Option<String>,
    },
    Split {
        target: Option<String>,
        direction: Direction,
        percent: Option<u8>,
        focus: bool,
        spawn: SpawnOpts,
        key: Option<String>,
    },
    NewWindow {
        focus: bool,
        spawn: SpawnOpts,
        key: Option<String>,
    },
    Focus(String),
    PipeCreate {
        name: String,
        capacity: Option<usize>,
    },
    PipeWrite {
        name: String,
        data: String,
        key: Option<String>,
    },
    PipeRead {
        name: String,
        args: PipeReadArgs,
    },
    PipeList,
    Sessions,
    Agents,
    Restore,
    AgentLaunch(AgentLaunch),
    AgentPrompt(PromptCall),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub harness: String,
    pub resume: Option<String>,
    pub cwd: Option<String>,
    pub split: bool,
    pub teams: bool,
    pub name: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCall {
    pub name: String,
    pub text: String,
    pub wait: bool,
    pub timeout_secs: u64,
    pub key: Option<String>,
}

fn text(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn required(args: &Value, key: &str) -> Result<String, String> {
    text(args, key).ok_or_else(|| format!("missing required argument: {key}"))
}

fn number(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn flag(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn list(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn default_name(command: &str) -> String {
    std::path::Path::new(command)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "task".to_string())
}

fn spawn_opts(args: &Value, default_pty: bool) -> Result<SpawnOpts, String> {
    let command = required(args, "command")?;
    Ok(SpawnOpts {
        name: text(args, "name").unwrap_or_else(|| default_name(&command)),
        command,
        args: list(args, "args"),
        pty: flag(args, "pty", default_pty),
        cwd: text(args, "cwd"),
        env: parse_env_pairs(&list(args, "env"))?,
        remain_on_exit: flag(args, "remain", false).then_some(true),
        tmux_compat: false,
    })
}

fn wait_condition(args: &Value) -> Result<(WaitCondition, Option<u64>), String> {
    let timeout_ms = number(args, "timeout").map(|s| s * 1000);
    let condition = match text(args, "condition").as_deref().unwrap_or("exit") {
        "exit" => WaitCondition::Exit,
        "idle" => WaitCondition::Idle {
            quiet_ms: number(args, "quiet_ms").unwrap_or(2000),
        },
        "match" => WaitCondition::Match {
            pattern: required(args, "pattern")?,
        },
        "state" => WaitCondition::State {
            state: parse_agent_state(&required(args, "state")?)?,
        },
        other => return Err(format!("unknown wait condition: {other}")),
    };
    Ok((condition, timeout_ms))
}

fn direction(args: &Value) -> Result<Direction, String> {
    match text(args, "direction").as_deref().unwrap_or("h") {
        "h" => Ok(Direction::Horizontal),
        "v" => Ok(Direction::Vertical),
        other => Err(format!("unknown direction: {other}")),
    }
}

/// Idempotency key of a call; an empty string means none.
fn idempotency_key(args: &Value) -> Option<String> {
    text(args, "idempotency_key").filter(|key| !key.is_empty())
}

/// Maps a `tools/call` name and arguments to a typed operation. Pure: no I/O.
pub fn parse_call(tool: &str, args: &Value) -> Result<Call, String> {
    let name = || required(args, "name");
    let key = || idempotency_key(args);
    Ok(match tool {
        "mpx_run" => Call::Run {
            spawn: spawn_opts(args, false)?,
            key: key(),
        },
        "mpx_list" => Call::List,
        "mpx_status" => Call::Status(name()?),
        "mpx_read" => Call::Read {
            name: name()?,
            tail: number(args, "tail").map(|n| n as usize),
            plain: flag(args, "plain", false),
            since: number(args, "since"),
        },
        "mpx_screen" => Call::Screen(name()?),
        "mpx_send" => Call::Send {
            name: name()?,
            data: required(args, "data")?,
            key: key(),
        },
        "mpx_send_keys" => Call::SendKeys {
            name: name()?,
            keys: list(args, "keys"),
            literal: flag(args, "literal", false),
            key: key(),
        },
        "mpx_paste" => Call::Paste {
            name: name()?,
            text: required(args, "text")?,
            submit: flag(args, "submit", false),
            key: key(),
        },
        "mpx_wait" => {
            let (condition, timeout_ms) = wait_condition(args)?;
            Call::Wait {
                name: name()?,
                condition,
                timeout_ms,
            }
        }
        "mpx_kill" => Call::Kill {
            name: name()?,
            key: key(),
        },
        "mpx_split" => Call::Split {
            target: text(args, "target"),
            direction: direction(args)?,
            percent: number(args, "percent").and_then(|p| u8::try_from(p).ok()),
            focus: flag(args, "focus", false),
            spawn: spawn_opts(args, false)?,
            key: key(),
        },
        "mpx_new_window" => Call::NewWindow {
            focus: flag(args, "focus", false),
            spawn: spawn_opts(args, false)?,
            key: key(),
        },
        "mpx_focus" => Call::Focus(name()?),
        "mpx_pipe_create" => Call::PipeCreate {
            name: name()?,
            capacity: number(args, "capacity").map(|n| n as usize),
        },
        "mpx_pipe_write" => Call::PipeWrite {
            name: name()?,
            data: required(args, "data")?,
            key: key(),
        },
        "mpx_pipe_read" => Call::PipeRead {
            name: name()?,
            args: PipeReadArgs {
                since: number(args, "since"),
                consumer: text(args, "consumer"),
                wait_ms: number(args, "wait_ms"),
                limit: number(args, "limit").map(|n| n as usize),
            },
        },
        "mpx_pipe_list" => Call::PipeList,
        "mpx_sessions" => Call::Sessions,
        "mpx_agents" => Call::Agents,
        "mpx_restore" => Call::Restore,
        "mpx_agent_launch" => Call::AgentLaunch(AgentLaunch {
            harness: required(args, "harness")?,
            resume: text(args, "resume"),
            cwd: text(args, "cwd"),
            split: flag(args, "split", false),
            teams: flag(args, "teams", false),
            name: text(args, "name"),
            key: key(),
        }),
        "mpx_agent_prompt" => Call::AgentPrompt(PromptCall {
            name: name()?,
            text: required(args, "text")?,
            wait: flag(args, "wait", true),
            timeout_secs: number(args, "timeout").unwrap_or(120),
            key: key(),
        }),
        other => return Err(format!("Unknown tool: {other}")),
    })
}

/// Names of every tool in the table.
pub fn tool_names() -> Vec<&'static str> {
    TOOLS.iter().map(|t| t.name).collect()
}
