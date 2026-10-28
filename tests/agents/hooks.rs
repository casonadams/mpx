use std::path::Path;

use mpx::agents::{
    HookReport, claude_hook_report, hook_report, omp_extension_source, omp_hook_report,
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
    ] {
        assert!(source.contains(&format!(r#"pi.on("{event}""#)), "{event}");
    }
    assert!(source.contains(r#"["hook", "omp", event]"#));
    assert!(source.contains("export default function"));
}

#[test]
fn hook_report_dispatches_by_harness() {
    assert!(hook_report("claude", None, &claude("Stop", "")).is_some());
    assert!(hook_report("omp", None, "{}").is_none());
    assert!(hook_report("omp", Some("agent_start"), "{}").is_some());
    assert!(hook_report("other", Some("agent_start"), "{}").is_none());
}
