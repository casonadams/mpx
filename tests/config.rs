use mpx::config::Config;

#[test]
fn partial_file_keeps_defaults_for_missing_keys() {
    let cfg = Config::parse("prefix = \"C-a\"\n[agent]\nidle_ms = 900\n").unwrap();
    assert_eq!(cfg.prefix, "C-a");
    assert_eq!(cfg.agent.idle_ms, 900);
    assert!(cfg.agent.notify);
    assert_eq!(cfg.history_limit, 64);
}

#[test]
fn unknown_keys_are_rejected() {
    assert!(Config::parse("prefx = \"C-a\"").is_err());
}

#[test]
fn bindings_and_harnesses_parse() {
    let text = r#"
[bind]
"M-h" = "select-pane -L"

[[harness]]
name = "codex"
detect = ["codex"]
command = ["codex"]
resume = ["codex", "resume", "{id}"]
"#;
    let cfg = Config::parse(text).unwrap();
    assert_eq!(
        cfg.bind.get("M-h").map(String::as_str),
        Some("select-pane -L")
    );
    assert_eq!(cfg.harness[0].resume, vec!["codex", "resume", "{id}"]);
}

#[test]
fn agents_get_mpx_tools_unless_turned_off() {
    assert!(Config::default().agent.mcp);
    let cfg = Config::parse("[agent]\nmcp = false\n").unwrap();
    assert!(!cfg.agent.mcp);
}
