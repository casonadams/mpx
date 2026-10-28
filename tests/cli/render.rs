use mpx::cli::render;
use mpx::protocol::{NodeInfo, PipeInfo, PipeMessage, StreamEvent};
use serde_json::Value;

fn node(id: u32, name: &str, pty: bool) -> NodeInfo {
    let mut n = NodeInfo::new(name.to_string(), pty, "sh".to_string());
    n.id = id;
    n
}

fn message(seq: u64, data: &str) -> PipeMessage {
    PipeMessage {
        seq,
        ts_ms: 0,
        data: data.to_string(),
    }
}

#[test]
fn pane_table_has_header_rule_and_one_row_per_node() {
    let mut exited = node(7, "server", false);
    exited.pid = Some(4242);
    let text = render::panes(&[node(3, "worker", true), exited], false).expect("table");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("ID") && lines[0].contains("COMMAND"));
    assert_eq!(lines[1], "-".repeat(86));
    assert!(lines[2].contains("%3") && lines[2].contains("worker") && lines[2].contains("pty"));
    assert!(lines[2].contains("running"));
    assert!(lines[3].contains("%7") && lines[3].contains("4242") && lines[3].contains("task"));
}

#[test]
fn pane_table_shows_dash_without_pid() {
    let text = render::panes(&[node(1, "idle", false)], false).expect("table");
    let row = text.lines().nth(2).expect("row");
    assert!(row.split_whitespace().any(|field| field == "-"));
}

#[test]
fn pane_json_is_the_node_array() {
    let text = render::panes(&[node(3, "worker", true)], true).expect("json");
    let value: Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(value[0]["name"], "worker");
    assert_eq!(value[0]["id"], 3);
}

#[test]
fn pipe_read_text_concatenates_message_data() {
    let text = render::pipe_read(&[message(1, "ab"), message(2, "cd")], 3, false).expect("text");
    assert_eq!(text, "abcd");
}

#[test]
fn pipe_read_json_reports_messages_and_next_offset() {
    let text = render::pipe_read(&[message(5, "hi")], 6, true).expect("json");
    assert!(text.ends_with('\n'));
    let value: Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(value["next"], 6);
    assert_eq!(value["messages"][0]["data"], "hi");
    assert_eq!(value["messages"][0]["seq"], 5);
}

#[test]
fn empty_pipe_list_names_the_session() {
    let text = render::pipes(&[], false, "work").expect("text");
    assert_eq!(text, "No virtual pipes in session 'work'\n");
}

#[test]
fn pipe_list_counts_and_names_each_pipe() {
    let pipes = vec![
        PipeInfo {
            name: "alpha".into(),
            messages: 0,
            next_seq: 1,
        },
        PipeInfo {
            name: "beta".into(),
            messages: 2,
            next_seq: 3,
        },
    ];
    let text = render::pipes(&pipes, false, "main").expect("text");
    assert!(text.starts_with("PIPES (2)\n"));
    assert!(text.contains("alpha\n") && text.contains("beta\n"));
}

#[test]
fn pipe_list_json_is_the_pipe_array() {
    let pipes = vec![PipeInfo {
        name: "alpha".into(),
        messages: 1,
        next_seq: 2,
    }];
    let value: Value =
        serde_json::from_str(&render::pipes(&pipes, true, "main").expect("json")).expect("valid");
    assert_eq!(value[0]["name"], "alpha");
}

#[test]
fn subscribe_prints_pipe_messages_only() {
    let pipe_event = StreamEvent::Pipe {
        name: "p".into(),
        message: message(1, "payload"),
    };
    assert_eq!(
        render::pipe_event(&pipe_event).as_deref(),
        Some("payload\n")
    );
    let output = StreamEvent::Output {
        name: "p".into(),
        data: "ignored".into(),
    };
    assert_eq!(render::pipe_event(&output), None);
}
