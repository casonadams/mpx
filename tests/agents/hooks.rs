use std::path::Path;

use mpx::agents::{
    HookReport, claude_hook_report, clip, hook_report, omp_extension_source, omp_hook_report,
    omp_plugin_dir,
};
use mpx::protocol::AgentState;

fn claude(event: &str, extra: &str) -> String {
    format!(r#"{{"session_id":"s-1","cwd":"/work","hook_event_name":"{event}"{extra}}}"#)
}

#[test]
fn claude_session_start_is_idle_and_carries_the_session() {
    let report = claude_hook_report(&claude("SessionStart", "")).expect("mapped");
    assert_eq!(report.state, Some(AgentState::Idle));
    assert_eq!(report.session_id.as_deref(), Some("s-1"));
    assert!(!report.clear);
}

#[test]
fn claude_prompt_and_tool_use_are_working() {
    for event in ["UserPromptSubmit", "PreToolUse"] {
        let report = claude_hook_report(&claude(event, "")).expect("mapped");
        assert_eq!(report.state, Some(AgentState::Working), "{event}");
    }
}

#[test]
fn claude_notification_needs_input_with_its_message() {
    let input = claude(
        "Notification",
        r#","message":"Claude needs your permission""#,
    );
    let report = claude_hook_report(&input).expect("mapped");
    assert_eq!(report.state, Some(AgentState::NeedsInput));
    assert_eq!(
        report.detail.as_deref(),
        Some("Claude needs your permission")
    );
}

#[test]
fn claude_stop_is_idle_and_session_end_clears() {
    assert_eq!(
        claude_hook_report(&claude("Stop", "")).and_then(|r| r.state),
        Some(AgentState::Idle)
    );
    let end = claude_hook_report(&claude("SessionEnd", "")).expect("mapped");
    assert!(end.clear);
    assert_eq!(end.state, None);
}

#[test]
fn claude_unknown_events_and_bad_json_are_ignored() {
    assert!(claude_hook_report(&claude("PostToolUse", "")).is_none());
    assert!(claude_hook_report("not json").is_none());
    assert!(claude_hook_report(r#"{"session_id":"x"}"#).is_none());
}

#[test]
fn omp_events_map_to_states() {
    let input = r#"{"session_id":"o-1"}"#;
    assert_eq!(
        omp_hook_report("session_start", input),
        Some(HookReport {
            state: Some(AgentState::Idle),
            session_id: Some("o-1".into()),
            detail: None,
            clear: false,
            ..HookReport::default()
        })
    );
    for event in ["agent_start", "turn_start", "tool_approval_resolved"] {
        assert_eq!(
            omp_hook_report(event, "{}").and_then(|r| r.state),
            Some(AgentState::Working),
            "{event}"
        );
    }
    assert_eq!(
        omp_hook_report("agent_end", "{}").and_then(|r| r.state),
        Some(AgentState::Idle)
    );
    assert!(
        omp_hook_report("turn_end", "{}").is_none(),
        "a turn ends many times inside one run; it must not mark the agent idle"
    );
    let approval =
        omp_hook_report("tool_approval_requested", r#"{"detail":"bash"}"#).expect("mapped");
    assert_eq!(approval.state, Some(AgentState::NeedsInput));
    assert_eq!(approval.detail.as_deref(), Some("bash"));
    assert!(omp_hook_report("session_shutdown", "{}").is_some_and(|r| r.clear));
    assert!(omp_hook_report("unknown_event", "{}").is_none());
}

#[test]
fn omp_extension_forwards_events_to_the_given_binary() {
    let source = omp_extension_source(Path::new("/opt/mpx/bin/mpx"));
    assert!(source.contains(r#"const MPX = "/opt/mpx/bin/mpx";"#));
    for event in [
        "session_start",
        "agent_start",
        "agent_end",
        "tool_approval_requested",
        "tool_approval_resolved",
        "session_shutdown",
        "before_agent_start",
        "tool_result",
    ] {
        assert!(source.contains(&format!(r#"pi.on("{event}""#)), "{event}");
    }
    assert!(source.contains(r#"["hook", "omp", event]"#));
    assert!(source.contains("additionalContext"), "file notices reach the model");
    assert!(source.contains(r#"customType: "mpx""#), "the roster reaches the model");
    assert!(source.contains("export default function"));
}

#[test]
fn hook_report_dispatches_by_harness() {
    assert!(hook_report("claude", None, &claude("Stop", "")).is_some());
    assert!(hook_report("omp", None, "{}").is_none());
    assert!(hook_report("omp", Some("agent_start"), "{}").is_some());
    assert!(hook_report("other", Some("agent_start"), "{}").is_none());
}

#[test]
fn claude_prompt_becomes_the_task_on_one_clipped_line() {
    let long = "x".repeat(400);
    let input = claude(
        "UserPromptSubmit",
        &format!(r#","prompt":"fix the\n  auth   bug {long}""#),
    );
    let task = claude_hook_report(&input).and_then(|r| r.task).expect("task");
    assert!(task.starts_with("fix the auth bug x"), "{task}");
    assert_eq!(task.chars().count(), 240);
    assert!(task.ends_with('…'));
}

#[test]
fn claude_stop_keeps_the_last_reply() {
    let input = claude("Stop", r#","last_assistant_message":"All tests pass.""#);
    let report = claude_hook_report(&input).expect("mapped");
    assert_eq!(report.state, Some(AgentState::Idle));
    assert_eq!(report.reply.as_deref(), Some("All tests pass."));
}

#[test]
fn claude_post_tool_use_records_edited_files_only() {
    let edit = claude(
        "PostToolUse",
        r#","tool_name":"Edit","tool_input":{"file_path":"/work/src/a.rs"}"#,
    );
    let report = claude_hook_report(&edit).expect("edit recorded");
    assert_eq!(report.edited.as_deref(), Some("/work/src/a.rs"));
    assert_eq!(report.state, None, "an edit does not change the agent state");

    let notebook = claude(
        "PostToolUse",
        r#","tool_name":"NotebookEdit","tool_input":{"notebook_path":"/work/n.ipynb"}"#,
    );
    let report = claude_hook_report(&notebook).expect("notebook recorded");
    assert_eq!(report.edited.as_deref(), Some("/work/n.ipynb"));

    let read = claude(
        "PostToolUse",
        r#","tool_name":"Read","tool_input":{"file_path":"/work/src/a.rs"}"#,
    );
    assert!(claude_hook_report(&read).is_none());
}

#[test]
fn clip_flattens_whitespace_and_marks_cuts() {
    assert_eq!(clip("  a\n\tb  c ", 10), "a b c");
    assert_eq!(clip("abcdef", 4), "abc…");
}

#[test]
fn omp_shares_its_task_reply_and_edits_like_claude() {
    let prompt = omp_hook_report("before_agent_start", r#"{"prompt":"fix\n the  login bug"}"#)
        .expect("mapped");
    assert_eq!(prompt.state, Some(AgentState::Working));
    assert_eq!(prompt.task.as_deref(), Some("fix the login bug"));

    let done = omp_hook_report("agent_end", r#"{"reply":"Fixed it."}"#).expect("mapped");
    assert_eq!(done.state, Some(AgentState::Idle));
    assert_eq!(done.reply.as_deref(), Some("Fixed it."));

    let wrote = omp_hook_report("tool_result", r#"{"tool":"write","path":"/r/a.rs"}"#);
    assert_eq!(wrote.and_then(|r| r.edited).as_deref(), Some("/r/a.rs"));
    let edited = omp_hook_report("tool_result", r#"{"tool":"edit","path":"/r/b.rs"}"#);
    assert_eq!(edited.and_then(|r| r.edited).as_deref(), Some("/r/b.rs"));
    assert!(
        omp_hook_report("tool_result", r#"{"tool":"read","path":"/r/a.rs"}"#).is_none(),
        "reading changes nothing"
    );
    let mcp_call = r#"{"tool":"write","path":"xd://mcp__mpx_mpx_mpx_peers"}"#;
    assert!(
        omp_hook_report("tool_result", mcp_call).is_none(),
        "omp runs MCP tools by writing to a virtual path, which is no edit"
    );
}

#[test]
fn omp_plugin_gives_the_agent_mpx_mcp_tools_through_a_script_in_the_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plugin = omp_plugin_dir(dir.path(), Path::new("/opt/my apps/mpx")).expect("written");
    let read = |name: &str| std::fs::read_to_string(plugin.join(name)).expect(name);
    let mcp: serde_json::Value = serde_json::from_str(&read("mcp.json")).expect("json");
    assert_eq!(mcp["mcpServers"]["mpx"]["command"], "./mpx", "inside the plugin, as omp requires");
    assert_eq!(mcp["mcpServers"]["mpx"]["args"], serde_json::json!(["mcp"]));
    let manifest: serde_json::Value = serde_json::from_str(&read("plugin.json")).expect("json");
    assert_eq!(manifest["name"], "mpx");
    assert!(read("mpx").contains("exec '/opt/my apps/mpx' \"$@\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(plugin.join("mpx")).expect("script").permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable");
    }
}
