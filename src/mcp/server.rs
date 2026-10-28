use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

use super::exec::execute;
use super::tools::{Call, parse_call, tool_list};
use crate::agents::aware::Me;
use crate::client::Client;
use crate::config::Config;

const PROTOCOL_VERSION: &str = "2024-11-05";
const METHOD_NOT_FOUND: i64 = -32601;
const PARSE_ERROR: i64 = -32700;

pub struct McpServer {
    endpoint: String,
    session: String,
    socket_dir: PathBuf,
    config: Config,
    caller: Option<Me>,
}

impl McpServer {
    /// `socket_dir` holds the session sockets; it is where other sessions are discovered. `config`
    /// supplies the agent harness registry.
    pub fn new(endpoint: String, session: String, socket_dir: PathBuf, config: Config) -> Self {
        Self {
            endpoint,
            session,
            socket_dir,
            config,
            caller: None,
        }
    }

    /// The agent this server runs for, so `mpx_peers` can leave it out and find its repository.
    pub fn with_caller(mut self, caller: Option<Me>) -> Self {
        self.caller = caller;
        self
    }

    /// Serves JSON-RPC over stdin/stdout until stdin closes.
    pub async fn run_stdio(&self) -> io::Result<()> {
        self.serve(io::stdin().lock(), io::stdout()).await
    }

    /// Serves newline-delimited JSON-RPC from `input` to `output` until `input` ends.
    /// Blank lines are skipped and notifications (no `id`) get no response.
    pub async fn serve<R: BufRead, W: Write>(&self, input: R, mut output: W) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle_line(&line).await {
                writeln!(output, "{response}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Response for one request line; notifications (no id) produce none.
    async fn handle_line(&self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(msg) => msg,
            Err(e) => {
                return Some(error_response(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("Parse error: {e}"),
                ));
            }
        };
        let id = msg.get("id").cloned()?;
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        Some(match self.handle(method, &params).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    async fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(self.initialize()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_list() })),
            "tools/call" => Ok(self.tools_call(params).await),
            other => Err((METHOD_NOT_FOUND, format!("Method '{other}' not found"))),
        }
    }

    fn initialize(&self) -> Value {
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "mpx",
                "version": env!("CARGO_PKG_VERSION"),
                "session": self.session,
            },
        })
    }

    async fn tools_call(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match self.call_tool(name, &args).await {
            Ok(value) => tool_result(&value, false),
            Err(message) => tool_result(&Value::String(message), true),
        }
    }

    async fn call_tool(&self, name: &str, args: &Value) -> Result<Value, String> {
        let call = match parse_call(name, args)? {
            Call::Peers { all, .. } => Call::Peers {
                all,
                me: self.caller.clone(),
            },
            call => call,
        };
        let mut client = Client::connect(&self.endpoint).await?;
        execute(&mut client, call, &self.socket_dir, &self.config).await
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// MCP tool result: the JSON value (or error text) as a single text content block.
pub fn tool_result(value: &Value, is_error: bool) -> Value {
    let text = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}
