use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use super::exec::execute;
use super::tools::{parse_call, tool_list};
use crate::client::Client;

const PROTOCOL_VERSION: &str = "2024-11-05";
const METHOD_NOT_FOUND: i64 = -32601;
const PARSE_ERROR: i64 = -32700;

pub struct McpServer {
    endpoint: String,
    session: String,
}

impl McpServer {
    pub fn new(endpoint: String, session: String) -> Self {
        Self { endpoint, session }
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
        let call = parse_call(name, args)?;
        let mut client = Client::connect(&self.endpoint).await?;
        execute(&mut client, call).await
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
