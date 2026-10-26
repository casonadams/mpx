use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use crate::client::Client;
use crate::layout::Direction;
use crate::protocol::SplitOpts;

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub struct McpServer {
    endpoint: String,
    session: String,
}

impl McpServer {
    pub fn new(endpoint: String, session: String) -> Self {
        Self { endpoint, session }
    }

    pub async fn run_stdio(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line_res in stdin.lock().lines() {
            let line = line_res?;
            if line.trim().is_empty() {
                continue;
            }

            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let err_resp = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                    });
                    writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                    stdout.flush()?;
                    continue;
                }
            };

            // Notifications have no id, do not respond
            let id = match req.id {
                Some(id) => id,
                None => continue,
            };

            let res = self.handle_method(&req.method, req.params).await;
            let resp = match res {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(result),
                    error: None,
                },
                Err(err_msg) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(json!({ "code": -32603, "message": err_msg })),
                },
            };

            writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
            stdout.flush()?;
        }

        Ok(())
    }

    async fn handle_method(&self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "mpx",
                    "version": "0.1.0",
                    "session": self.session
                },
                "capabilities": {
                    "tools": {}
                }
            })),

            "tools/list" => Ok(json!({
                "tools": [
                    {
                        "name": "mpx_run",
                        "description": "Spawn a task or command in a managed stream node",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "command": { "type": "string", "description": "Binary or command to execute" },
                                "args": { "type": "array", "items": { "type": "string" }, "description": "Command arguments" },
                                "name": { "type": "string", "description": "Name for the stream node" },
                                "pty": { "type": "boolean", "description": "Allocate interactive PTY (default false)" }
                            },
                            "required": ["command"]
                        }
                    },
                    {
                        "name": "mpx_split",
                        "description": "Split workspace pane and spawn a command",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "command": { "type": "string", "description": "Binary or command to execute" },
                                "args": { "type": "array", "items": { "type": "string" } },
                                "name": { "type": "string", "description": "Name for the stream node" },
                                "direction": { "type": "string", "enum": ["h", "v"], "description": "Split direction: horizontal (h) or vertical (v)" },
                                "pty": { "type": "boolean", "description": "Allocate interactive PTY" }
                            },
                            "required": ["command"]
                        }
                    },
                    {
                        "name": "mpx_list",
                        "description": "List all active stream nodes, their PIDs, types, and statuses",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "mpx_read",
                        "description": "Read scrollback logs from a stream node",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the node" },
                                "tail": { "type": "integer", "description": "Optional lines from tail" }
                            },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "mpx_screen",
                        "description": "Snapshot the 2D virtual terminal screen buffer of a node",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the node" }
                            },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "mpx_send",
                        "description": "Send input data string to a running node's stdin",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the node" },
                                "data": { "type": "string", "description": "Input text to write to stdin" }
                            },
                            "required": ["name", "data"]
                        }
                    },
                    {
                        "name": "mpx_wait",
                        "description": "Wait for a node to exit and return its exit code",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the node" }
                            },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "mpx_kill",
                        "description": "Terminate a running stream node",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the node" }
                            },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "mpx_pipe_write",
                        "description": "Write message or tokens into a named inter-agent virtual pipe",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the virtual pipe" },
                                "data": { "type": "string", "description": "Content string to append" }
                            },
                            "required": ["name", "data"]
                        }
                    },
                    {
                        "name": "mpx_pipe_read",
                        "description": "Read all content accumulated in an inter-agent virtual pipe",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Name of the virtual pipe" }
                            },
                            "required": ["name"]
                        }
                    }
                ]
            })),

            "tools/call" => {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                self.call_tool(tool_name, args).await
            }

            _ => Err(format!("Method '{}' not found", method)),
        }
    }

    async fn call_tool(&self, tool: &str, args: Value) -> Result<Value, String> {
        let mut client = Client::connect(&self.endpoint).await?;

        match tool {
            "mpx_run" => {
                let command = args.get("command").and_then(|v| v.as_str()).ok_or("Missing command")?;
                let node_args: Vec<String> = args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        std::path::Path::new(command)
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "task".into())
                    });
                let pty = args.get("pty").and_then(|v| v.as_bool()).unwrap_or(false);

                let info = client.spawn(name, command.to_string(), node_args, pty, None).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Node '{}' started (pid: {:?}, pty: {})", info.name, info.pid, info.pty) }]
                }))
            }

            "mpx_split" => {
                let command = args.get("command").and_then(|v| v.as_str()).ok_or("Missing command")?;
                let node_args: Vec<String> = args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| "split_pane".into());
                let dir_str = args.get("direction").and_then(|v| v.as_str()).unwrap_or("h");
                let dir = if dir_str.starts_with('v') {
                    Direction::Vertical
                } else {
                    Direction::Horizontal
                };
                let pty = args.get("pty").and_then(|v| v.as_bool()).unwrap_or(false);

                let info = client
                    .split(SplitOpts {
                        target: None,
                        name,
                        command: command.to_string(),
                        args: node_args,
                        pty,
                        direction: dir,
                        cwd: None,
                    })
                    .await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Split pane '{}' created (pid: {:?})", info.name, info.pid) }]
                }))
            }

            "mpx_list" => {
                let list = client.list().await?;
                let formatted = serde_json::to_string_pretty(&list).unwrap();
                Ok(json!({
                    "content": [{ "type": "text", "text": formatted }]
                }))
            }

            "mpx_read" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let tail = args.get("tail").and_then(|v| v.as_u64()).map(|u| u as usize);
                let logs = client.logs(name, tail).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": logs }]
                }))
            }

            "mpx_screen" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let screen = client.screen(name).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": screen }]
                }))
            }

            "mpx_send" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let data = args.get("data").and_then(|v| v.as_str()).ok_or("Missing data")?;
                let payload = if data.ends_with('\n') { data.to_string() } else { format!("{}\n", data) };
                client.send_input(name, &payload).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Input sent to '{}'", name) }]
                }))
            }

            "mpx_wait" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let code = client.wait(name).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Node '{}' exited with code {:?}", name, code) }]
                }))
            }

            "mpx_kill" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                client.kill(name).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Node '{}' terminated", name) }]
                }))
            }

            "mpx_pipe_write" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let data = args.get("data").and_then(|v| v.as_str()).ok_or("Missing data")?;
                let _ = client.create_pipe(name, None).await; // idempotent
                client.write_pipe(name, data).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": format!("Wrote {} bytes to pipe '{}'", data.len(), name) }]
                }))
            }

            "mpx_pipe_read" => {
                let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing name")?;
                let text = client.read_pipe(name).await?;
                Ok(json!({
                    "content": [{ "type": "text", "text": text }]
                }))
            }

            _ => Err(format!("Unknown tool: {}", tool)),
        }
    }
}
