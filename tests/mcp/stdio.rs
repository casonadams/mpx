use mpx::config::Config;
use mpx::mcp::McpServer;
use serde_json::Value;

async fn serve(input: &str) -> Vec<Value> {
    let server = McpServer::new(
        "/nonexistent/mpx.sock".into(),
        "dev".into(),
        "/nonexistent".into(),
        Config::default(),
    );
    let mut output = Vec::new();
    server.serve(input.as_bytes(), &mut output).await.unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn answers_requests_in_order_and_skips_notifications_and_blank_lines() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        "\n",
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
    );
    let replies = serve(input).await;
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[0]["result"]["serverInfo"]["session"], "dev");
    assert_eq!(replies[1]["id"], 2);
    assert!(
        replies[1]["result"]["tools"]
            .as_array()
            .is_some_and(|t| !t.is_empty())
    );
}

#[tokio::test]
async fn malformed_json_and_unknown_methods_get_json_rpc_errors() {
    let input = "not json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"nope\"}\n";
    let replies = serve(input).await;
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["id"], Value::Null);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 7);
    assert_eq!(replies[1]["error"]["code"], -32601);
}
