use mpx::cli::render::{self, FollowStep, follow_step};
use mpx::protocol::NodeInfo;
use serde_json::Value;

fn node(id: u32, name: &str, pty: bool) -> NodeInfo {
    let mut n = NodeInfo::new(name.to_string(), pty, "cmd".to_string());
    n.id = id;
    n
}

#[test]
fn pane_table_has_header_rule_and_one_row_per_node() {
    let nodes = [node(0, "first", true), node(1, "second", false)];
    let out = render::panes(&nodes, false).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("ID"));
    assert!(lines[1].chars().all(|c| c == '-'));
    assert!(lines[2].contains("%0"));
    assert!(lines[2].contains("first"));
    assert!(lines[2].contains("pty"));
    assert!(lines[3].contains("%1"));
    assert!(lines[3].contains("second"));
    assert!(lines[3].contains("task"));
}

#[test]
fn pane_table_shows_dash_without_pid() {
    let nodes = [node(0, "first", true)];
    let out = render::panes(&nodes, false).unwrap();
    assert!(out.lines().nth(2).unwrap().contains(" - "));
}

#[test]
fn pane_json_is_the_node_array() {
    let nodes = [node(0, "first", true)];
    let value: Value = serde_json::from_str(&render::panes(&nodes, true).unwrap()).unwrap();
    assert_eq!(value[0]["name"], "first");
    assert_eq!(value[0]["id"], 0);
}

#[test]
fn follow_prints_the_chunk_that_continues_the_output() {
    assert_eq!(follow_step(10, 10, 14), FollowStep::Print);
}

#[test]
fn follow_skips_chunks_already_printed() {
    assert_eq!(follow_step(10, 5, 8), FollowStep::Skip);
}

#[test]
fn follow_resyncs_after_a_gap_or_a_straddling_chunk() {
    assert_eq!(follow_step(10, 12, 14), FollowStep::Resync);
    assert_eq!(follow_step(10, 8, 14), FollowStep::Resync);
}
