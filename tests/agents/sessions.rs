use mpx::agents::{parse_claude_meta, parse_codex_meta, parse_omp_meta, parse_preview};

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
    let meta = parse_claude_meta(head, "file-stem");
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
    let meta = parse_claude_meta(&head, "file-stem");
    assert_eq!(meta.title.chars().count(), 80);
}

#[test]
fn claude_skips_injected_context_when_picking_a_prompt() {
    let head = concat!(
        r#"{"type":"user","message":{"role":"user","content":"<system-reminder>env</system-reminder>"}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Real question"}]}}"#,
    );
    let meta = parse_claude_meta(head, "stem");
    assert_eq!(meta.title, "Real question");
}

#[test]
fn claude_without_any_fields_uses_file_stem_as_id() {
    let meta = parse_claude_meta("not json at all", "stem-only");
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
fn preview_limit_zero_is_empty() {
    assert!(
        parse_preview(
            r#"{"type":"user","message":{"role":"user","content":"x"}}"#,
            0
        )
        .is_empty()
    );
}
