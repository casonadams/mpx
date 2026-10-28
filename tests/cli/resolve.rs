use mpx::cli::{
    nested_attach_refused, parse_agent_state, parse_env_pairs, resolve, resolve_idempotency_key,
    wait_condition, wait_exit_code,
};
use mpx::ipc::default_endpoint;
use mpx::protocol::{AgentState, WaitCondition, WaitOutcome};

fn outcome(satisfied: bool, exit_code: Option<i32>) -> WaitOutcome {
    WaitOutcome {
        satisfied,
        exit_code,
        state: None,
        matched: None,
    }
}

#[test]
fn session_defaults_to_main() {
    let r = resolve(None, None, None, None);
    assert_eq!(r.session, "main");
    assert_eq!(r.endpoint, default_endpoint("main"));
}

#[test]
fn session_flag_beats_environment() {
    assert_eq!(resolve(Some("a"), None, Some("b"), None).session, "a");
    assert_eq!(resolve(None, None, Some("b"), None).session, "b");
}

#[test]
fn environment_endpoint_applies_only_without_session_flag() {
    assert_eq!(
        resolve(None, None, None, Some("/env.sock")).endpoint,
        "/env.sock"
    );
    assert_eq!(
        resolve(Some("a"), None, None, Some("/env.sock")).endpoint,
        default_endpoint("a")
    );
}

#[test]
fn explicit_endpoint_beats_everything() {
    let r = resolve(Some("a"), Some("/x.sock"), Some("b"), Some("/env.sock"));
    assert_eq!(r.endpoint, "/x.sock");
    assert_eq!(r.session, "a");
}

#[test]
fn env_pairs_split_on_the_first_equals_sign() {
    let pairs = parse_env_pairs(&["A=1".to_string(), "B=x=y".to_string(), "EMPTY=".to_string()])
        .expect("valid");
    assert_eq!(
        pairs,
        [
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "x=y".to_string()),
            ("EMPTY".to_string(), String::new()),
        ]
    );
}

#[test]
fn env_pair_without_equals_is_rejected() {
    assert!(parse_env_pairs(&["NOEQ".to_string()]).is_err());
}

#[test]
fn agent_state_names_parse() {
    assert_eq!(parse_agent_state("needs_input"), Ok(AgentState::NeedsInput));
    assert_eq!(parse_agent_state("working"), Ok(AgentState::Working));
    assert_eq!(parse_agent_state("idle"), Ok(AgentState::Idle));
    assert_eq!(parse_agent_state("exited"), Ok(AgentState::Exited));
    assert!(parse_agent_state("busy").is_err());
}

#[test]
fn wait_without_options_waits_for_exit() {
    assert_eq!(wait_condition(None, None, None), Ok(WaitCondition::Exit));
}

#[test]
fn wait_options_select_one_condition() {
    assert_eq!(
        wait_condition(Some(500), None, None),
        Ok(WaitCondition::Idle { quiet_ms: 500 })
    );
    assert_eq!(
        wait_condition(None, Some("ready"), None),
        Ok(WaitCondition::Match {
            pattern: "ready".into()
        })
    );
    assert_eq!(
        wait_condition(None, None, Some("idle")),
        Ok(WaitCondition::State {
            state: AgentState::Idle
        })
    );
}

#[test]
fn wait_rejects_combined_or_invalid_conditions() {
    assert!(wait_condition(Some(1), Some("x"), None).is_err());
    assert!(wait_condition(None, Some("(unclosed"), None).is_err());
    assert!(wait_condition(None, None, Some("nope")).is_err());
}

#[test]
fn exit_wait_reports_the_child_code() {
    assert_eq!(
        wait_exit_code(&WaitCondition::Exit, &outcome(true, Some(7))),
        7
    );
    assert_eq!(
        wait_exit_code(&WaitCondition::Exit, &outcome(true, None)),
        0
    );
}

#[test]
fn timeout_is_124_for_every_condition() {
    assert_eq!(
        wait_exit_code(&WaitCondition::Exit, &outcome(false, None)),
        124
    );
    let idle = WaitCondition::Idle { quiet_ms: 10 };
    assert_eq!(wait_exit_code(&idle, &outcome(false, None)), 124);
}

#[test]
fn satisfied_non_exit_conditions_exit_zero() {
    let idle = WaitCondition::Idle { quiet_ms: 10 };
    assert_eq!(wait_exit_code(&idle, &outcome(true, Some(3))), 0);
}

#[test]
fn nested_attach_is_refused_only_inside_mpx_without_force() {
    assert!(nested_attach_refused(true, false));
    assert!(!nested_attach_refused(true, true));
    assert!(!nested_attach_refused(false, false));
}

#[test]
fn idempotency_key_comes_from_the_flag_and_empty_is_unset() {
    assert_eq!(
        resolve_idempotency_key(Some("retry-1")).as_deref(),
        Some("retry-1")
    );
    assert_eq!(resolve_idempotency_key(Some("")), None);
    assert_eq!(resolve_idempotency_key(None), None);
}
