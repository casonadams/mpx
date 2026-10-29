use mpx::agents::{
    lookup_rho_cwd, parse_claude_meta, parse_codex_meta, parse_omp_meta, parse_opencode_meta,
    parse_preview, parse_rho_meta,
};

const OMP_HEAD: &str = concat!(
    r#"{"type":"title","title":"Fix the parser"}"#,
    "\n",
    r#"{"type":"session","id":"omp-1","cwd":"/work/project"}"#,
    "\n",
    r#"{"type":"model_change","model":"alpha"}"#,
);

#[test]
fn omp_header_gives_id_cwd_and_title() {
    let meta = parse_omp_meta(OMP_HEAD).expect("session header");
    assert_eq!(meta.id, "omp-1");
    assert_eq!(meta.cwd.as_deref(), Some("/work/project"));
    assert_eq!(meta.title, "Fix the parser");
}

#[test]
fn omp_without_title_falls_back_to_first_prompt() {
    let head = concat!(
        r#"{"type":"title","title":""}"#,
        "\n",
        r#"{"type":"session","id":"omp-2","cwd":"/w"}"#,
        "\n",
        r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"  hello there\nmore"}]}}"#,
    );
    let meta = parse_omp_meta(head).expect("session header");
    assert_eq!(meta.title, "hello there");
}

#[test]
fn omp_without_session_line_is_not_a_session() {
    assert!(parse_omp_meta(r#"{"type":"title","title":"x"}"#).is_none());
    assert!(parse_omp_meta("").is_none());
}

#[test]
fn claude_uses_session_id_cwd_and_summary_title() {
    let head = concat!(
        r#"{"type":"summary","summary":"Refactor the store","leafUuid":"u1"}"#,
        "\n",
        r#"{"sessionId":"claude-1","cwd":"/work/app","type":"user","message":{"role":"user","content":"ignored"}}"#,
    );
    let meta = parse_claude_meta(head, "", "file-stem");
    assert_eq!(meta.id, "claude-1");
    assert_eq!(meta.cwd.as_deref(), Some("/work/app"));
    assert_eq!(meta.title, "Refactor the store");
}

#[test]
fn claude_title_falls_back_to_first_prompt_truncated() {
    let long = "x".repeat(200);
    let head = format!(
        r#"{{"sessionId":"claude-2","cwd":"/a","type":"user","message":{{"role":"user","content":"{long}"}}}}"#
    );
    let meta = parse_claude_meta(&head, "", "file-stem");
    assert_eq!(meta.title.chars().count(), 80);
}

#[test]
fn claude_skips_injected_context_when_picking_a_prompt() {
    let head = concat!(
        r#"{"type":"user","message":{"role":"user","content":"<system-reminder>env</system-reminder>"}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Real question"}]}}"#,
    );
    let meta = parse_claude_meta(head, "", "stem");
    assert_eq!(meta.title, "Real question");
}

#[test]
fn claude_without_any_fields_uses_file_stem_as_id() {
    let meta = parse_claude_meta("not json at all", "", "stem-only");
    assert_eq!(meta.id, "stem-only");
    assert_eq!(meta.title, "stem-only");
    assert_eq!(meta.cwd, None);
}

#[test]
fn codex_session_meta_gives_id_cwd_and_first_prompt() {
    let head = concat!(
        r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"/code/service"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
        "\n",
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"Add a retry"}}"#,
    );
    let meta = parse_codex_meta(head).expect("session_meta");
    assert_eq!(meta.id, "codex-1");
    assert_eq!(meta.cwd.as_deref(), Some("/code/service"));
    assert_eq!(meta.title, "Add a retry");
}

#[test]
fn codex_requires_session_meta() {
    assert!(parse_codex_meta(r#"{"type":"event_msg","payload":{}}"#).is_none());
}

#[test]
fn rho_header_and_prompt_give_id_and_title() {
    let head = concat!(
        r#"{"record_type":"header","version":2,"session_id":"rho-sess-1","created_at":"2026-10-28T06:52:19Z"}"#,
        "\n",
        r#"{"record_type":"audit_event","sequence":1,"session_id":"rho-sess-1","event":{"id":"e1","timestamp":"2026-10-28T06:52:47Z","kind":"user_message","payload":{"prompt":"add rho support"}}}"#,
    );
    let meta = parse_rho_meta(head, "file-stem").expect("rho session");
    assert_eq!(meta.id, "rho-sess-1");
    assert_eq!(meta.title, "add rho support");
    assert_eq!(meta.cwd, None);
}

#[test]
fn rho_session_named_sets_explicit_title() {
    let head = concat!(
        r#"{"record_type":"header","version":2,"session_id":"rho-sess-2","created_at":"2026-10-28T06:52:19Z"}"#,
        "\n",
        r#"{"record_type":"session_named","sequence":1,"session_id":"rho-sess-2","timestamp":"2026-10-28T06:52:20Z","name":"Feature branch"}"#,
        "\n",
        r#"{"record_type":"audit_event","sequence":2,"session_id":"rho-sess-2","event":{"id":"e1","timestamp":"2026-10-28T06:52:47Z","kind":"user_message","payload":{"prompt":"first prompt"}}}"#,
    );
    let meta = parse_rho_meta(head, "file-stem").expect("rho session");
    assert_eq!(meta.id, "rho-sess-2");
    assert_eq!(meta.title, "Feature branch");
}

#[test]
fn rho_without_header_or_id_is_not_a_session() {
    assert!(parse_rho_meta(r#"{"type":"event_msg","payload":{}}"#, "stem").is_none());
    assert!(parse_rho_meta("", "stem").is_none());
}

#[test]
fn rho_explicit_cwd_in_meta() {
    let head = r#"{"record_type":"header","session_id":"rho-3","cwd":"/projects/mpx"}"#;
    let meta = parse_rho_meta(head, "stem").expect("session");
    assert_eq!(meta.cwd.as_deref(), Some("/projects/mpx"));
}

#[test]
fn rho_cwd_lookup_from_last_sessions_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let index = tmp.path().join(".last_sessions.json");
    let json = r#"{"/projects/mpx":"rho-sess-42","/other/repo":"rho-sess-99"}"#;
    std::fs::write(&index, json).expect("write index");

    let sess_file = tmp.path().join("rho-sess-42.jsonl");
    assert_eq!(
        lookup_rho_cwd(&sess_file, "rho-sess-42").as_deref(),
        Some("/projects/mpx")
    );
    assert_eq!(lookup_rho_cwd(&sess_file, "unknown"), None);
}

#[test]
fn opencode_session_meta_parses_fields_and_fallbacks() {
    let full = r#"{"id":"ses_4a309","projectID":"proj1","directory":"/p/lighten","title":"Fix navbar"}"#;
    let meta = parse_opencode_meta(full, "file-stem").expect("opencode session");
    assert_eq!(meta.id, "ses_4a309");
    assert_eq!(meta.title, "Fix navbar");
    assert_eq!(meta.cwd.as_deref(), Some("/p/lighten"));

    let minimal = r#"{"id":"ses_123","version":"1.0"}"#;
    let meta2 = parse_opencode_meta(minimal, "file-stem").expect("minimal opencode");
    assert_eq!(meta2.id, "ses_123");
    assert_eq!(meta2.title, "ses_123");
    assert_eq!(meta2.cwd, None);

    let from_stem = r#"{"projectID":"proj_abc","title":"Working branch"}"#;
    let meta3 = parse_opencode_meta(from_stem, "ses_stem").expect("stem fallback");
    assert_eq!(meta3.id, "ses_stem");
    assert_eq!(meta3.title, "Working branch");
}

#[test]
fn opencode_non_opencode_json_is_none() {
    assert!(parse_opencode_meta("{}", "stem").is_none());
    assert!(parse_opencode_meta(r#"{"unknown":"data"}"#, "stem").is_none());
    assert!(parse_opencode_meta("", "stem").is_none());
}

#[test]
fn preview_keeps_the_last_user_and_assistant_messages() {
    let text = concat!(
        r#"{"type":"user","message":{"role":"user","content":"first question"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first answer"}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"second question"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"x"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"line one\nline two"}]}}"#,
    );
    let lines = parse_preview(text, 3);
    assert_eq!(
        lines,
        ["user> second question", "assistant> line one", "  line two"]
    );
}

#[test]
fn preview_skips_system_roles_and_bad_lines() {
    let text = concat!(
        "garbage\n",
        r#"{"type":"system","message":{"role":"system","content":"ignored"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":"kept"}}"#,
    );
    assert_eq!(parse_preview(text, 10), ["assistant> kept"]);
}

#[test]
fn preview_of_codex_events_uses_user_messages() {
    let text = r#"{"type":"event_msg","payload":{"type":"user_message","message":"ping"}}"#;
    assert_eq!(parse_preview(text, 5), ["user> ping"]);
}

#[test]
fn preview_of_rho_audit_events_extracts_user_and_assistant() {
    let text = concat!(
        r#"{"record_type":"header","version":2,"session_id":"rho-1","created_at":"2026-10-28T06:52:19Z"}"#,
        "\n",
        r#"{"record_type":"audit_event","sequence":1,"session_id":"rho-1","event":{"id":"e1","timestamp":"2026-10-28T06:52:47Z","kind":"user_message","payload":{"prompt":"how does recovery work?"}}}"#,
        "\n",
        r#"{"record_type":"audit_event","sequence":2,"session_id":"rho-1","event":{"id":"e2","timestamp":"2026-10-28T06:52:48Z","kind":"assistant_response","payload":{"content":"via checkpoints and snapshots"}}}"#,
    );
    let lines = parse_preview(text, 10);
    assert_eq!(
        lines,
        [
            "user> how does recovery work?",
            "assistant> via checkpoints and snapshots"
        ]
    );
}

#[test]
fn preview_limit_zero_is_empty() {
    assert!(
        parse_preview(
            r#"{"type":"user","message":{"role":"user","content":"x"}}"#,
            0
        )
        .is_empty()
    );
}

#[test]
fn claude_title_prefers_the_latest_rename_then_the_latest_generated_title_from_the_tail() {
    let head =
        r#"{"sessionId":"c","type":"user","message":{"role":"user","content":"first prompt"}}"#;
    let generated = concat!(
        r#"{"type":"ai-title","aiTitle":"Old title","sessionId":"c"}"#,
        "\n",
        r#"{"type":"ai-title","aiTitle":"Billing branch review","sessionId":"c"}"#,
    );
    assert_eq!(
        parse_claude_meta(head, generated, "stem").title,
        "Billing branch review"
    );
    let renamed = format!(
        "{generated}\n{}",
        r#"{"type":"custom-title","customTitle":"My name","sessionId":"c"}"#
    );
    assert_eq!(parse_claude_meta(head, &renamed, "stem").title, "My name");
}
