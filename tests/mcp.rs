mod mcp {
    mod poll;
}

use mpx::layout::Direction;
use mpx::mcp::{Call, parse_call, tool_list, tool_names};
use mpx::protocol::{AgentState, WaitCondition};
use serde_json::{Value, json};

const EXPECTED: [&str; 21] = [
    "mpx_run",
    "mpx_list",
    "mpx_status",
    "mpx_read",
    "mpx_screen",
    "mpx_send",
    "mpx_send_keys",
    "mpx_paste",
    "mpx_wait",
    "mpx_kill",
    "mpx_split",
    "mpx_new_window",
    "mpx_focus",
    "mpx_pipe_create",
    "mpx_pipe_write",
    "mpx_pipe_read",
    "mpx_pipe_list",
    "mpx_sessions",
    "mpx_agents",
    "mpx_agent_launch",
    "mpx_agent_prompt",
];

#[test]
fn every_expected_tool_is_listed_once() {
    let mut names = tool_names();
    names.sort_unstable();
    let mut expected = EXPECTED.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[test]
fn tools_list_schemas_are_objects_with_known_required_fields() {
    let list = tool_list();
    let tools = list.as_array().expect("array of tools");
    assert_eq!(tools.len(), EXPECTED.len());
    for tool in tools {
        let name = tool["name"].as_str().expect("name");
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name}");
        assert!(
            tool["description"].as_str().is_some_and(|d| !d.is_empty()),
            "{name}"
        );
        let props = schema["properties"].as_object().expect("properties");
        for req in schema["required"].as_array().expect("required") {
            assert!(
                props.contains_key(req.as_str().unwrap_or("")),
                "{name} requires unknown {req}"
            );
        }
    }
}

#[test]
fn enum_properties_list_their_values() {
    let list = tool_list();
    let wait = find(&list, "mpx_wait");
    let condition = &wait["inputSchema"]["properties"]["condition"];
    assert_eq!(condition["enum"], json!(["exit", "idle", "match", "state"]));
    let split = find(&list, "mpx_split");
    assert_eq!(
        split["inputSchema"]["properties"]["direction"]["enum"],
        json!(["h", "v"])
    );
}

fn find<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .and_then(|tools| tools.iter().find(|t| t["name"] == name))
        .expect("tool present")
}

#[test]
fn run_maps_command_args_env_and_default_name() {
    let call = parse_call(
        "mpx_run",
        &json!({"command": "/usr/bin/tail", "args": ["-f", "log"], "env": ["A=1"], "remain": true}),
    )
    .expect("valid");
    let Call::Run(spawn) = call else {
        panic!("expected Run");
    };
    assert_eq!(spawn.name, "tail");
    assert_eq!(spawn.args, ["-f", "log"]);
    assert_eq!(spawn.env, [("A".to_string(), "1".to_string())]);
    assert_eq!(spawn.remain_on_exit, Some(true));
}

#[test]
fn missing_required_arguments_are_errors() {
    assert!(parse_call("mpx_run", &json!({})).is_err());
    assert!(parse_call("mpx_send", &json!({"name": "n"})).is_err());
    assert!(parse_call("mpx_status", &json!({})).is_err());
}

#[test]
fn unknown_tool_is_an_error() {
    assert!(parse_call("mpx_nope", &json!({})).is_err());
}

#[test]
fn wait_defaults_to_exit_and_converts_timeout_to_millis() {
    let call = parse_call("mpx_wait", &json!({"name": "n", "timeout": 3})).expect("valid");
    let Call::Wait {
        condition,
        timeout_ms,
        ..
    } = call
    else {
        panic!("expected Wait");
    };
    assert_eq!(condition, WaitCondition::Exit);
    assert_eq!(timeout_ms, Some(3000));
}

#[test]
fn wait_idle_match_and_state_conditions() {
    let idle = parse_call(
        "mpx_wait",
        &json!({"name": "n", "condition": "idle", "quiet_ms": 500}),
    )
    .expect("valid");
    assert!(matches!(
        idle,
        Call::Wait {
            condition: WaitCondition::Idle { quiet_ms: 500 },
            timeout_ms: None,
            ..
        }
    ));
    let matched = parse_call(
        "mpx_wait",
        &json!({"name": "n", "condition": "match", "pattern": "done"}),
    )
    .expect("valid");
    assert!(
        matches!(matched, Call::Wait { condition: WaitCondition::Match { ref pattern }, .. } if pattern == "done")
    );
    let state = parse_call(
        "mpx_wait",
        &json!({"name": "n", "condition": "state", "state": "needs_input"}),
    )
    .expect("valid");
    assert!(matches!(
        state,
        Call::Wait {
            condition: WaitCondition::State {
                state: AgentState::NeedsInput
            },
            ..
        }
    ));
}

#[test]
fn wait_rejects_incomplete_or_unknown_conditions() {
    assert!(parse_call("mpx_wait", &json!({"name": "n", "condition": "match"})).is_err());
    assert!(
        parse_call(
            "mpx_wait",
            &json!({"name": "n", "condition": "state", "state": "busy"})
        )
        .is_err()
    );
    assert!(parse_call("mpx_wait", &json!({"name": "n", "condition": "forever"})).is_err());
}

#[test]
fn read_maps_tail_plain_and_since() {
    let call = parse_call(
        "mpx_read",
        &json!({"name": "n", "tail": 10, "since": 42, "plain": true}),
    )
    .expect("valid");
    assert!(matches!(
        call,
        Call::Read {
            tail: Some(10),
            since: Some(42),
            plain: true,
            ..
        }
    ));
}

#[test]
fn split_maps_direction_percent_and_focus_default() {
    let call = parse_call(
        "mpx_split",
        &json!({"command": "x", "direction": "v", "percent": 30}),
    )
    .expect("valid");
    let Call::Split {
        direction,
        percent,
        focus,
        ..
    } = call
    else {
        panic!("expected Split");
    };
    assert_eq!(direction, Direction::Vertical);
    assert_eq!(percent, Some(30));
    assert!(!focus);
    assert!(parse_call("mpx_split", &json!({"command": "x", "direction": "q"})).is_err());
}

#[test]
fn pipe_read_maps_cursor_wait_and_limit() {
    let call = parse_call(
        "mpx_pipe_read",
        &json!({"name": "p", "consumer": "c", "wait_ms": 100, "limit": 5, "since": 7}),
    )
    .expect("valid");
    let Call::PipeRead { name, args } = call else {
        panic!("expected PipeRead");
    };
    assert_eq!(name, "p");
    assert_eq!(args.consumer.as_deref(), Some("c"));
    assert_eq!(args.wait_ms, Some(100));
    assert_eq!(args.limit, Some(5));
    assert_eq!(args.since, Some(7));
}

#[test]
fn agent_prompt_waits_by_default_for_two_minutes() {
    let call = parse_call("mpx_agent_prompt", &json!({"name": "n", "text": "hi"})).expect("valid");
    assert!(matches!(
        call,
        Call::AgentPrompt {
            wait: true,
            timeout_secs: 120,
            ..
        }
    ));
}

#[test]
fn agent_launch_maps_flags() {
    let call = parse_call(
        "mpx_agent_launch",
        &json!({"harness": "claude", "resume": "abc", "split": true, "teams": true}),
    )
    .expect("valid");
    let Call::AgentLaunch(launch) = call else {
        panic!("expected AgentLaunch");
    };
    assert_eq!(launch.harness, "claude");
    assert_eq!(launch.resume.as_deref(), Some("abc"));
    assert!(launch.split && launch.teams);
    assert!(parse_call("mpx_agent_launch", &json!({})).is_err());
}
