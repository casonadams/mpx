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
fn parse_style_dim_factor() {
    let text = "[style]\ndim_inactive = false\ndim_factor = 0.05\n";
    let cfg = Config::parse(text).unwrap();
    assert!(!cfg.style.dim_inactive);
    assert_eq!(cfg.style.dim_factor, 0.05);
}

#[test]
fn parse_style_active_inactive_templates() {
    let text = r##"
[style]
dim_inactive = true

[style.active]
fg = "#d8dee9"
bg = "#2e3440"

[style.inactive]
fg = "#4c566a"
bg = "#242933"
color_mode = "desaturate"
factor = 0.35
dim_content_bg = false
"##;
    let cfg = Config::parse(text).unwrap();
    assert_eq!(cfg.style.active.fg, "#d8dee9");
    assert_eq!(cfg.style.active.bg, "#2e3440");
    assert_eq!(cfg.style.inactive.fg, "#4c566a");
    assert_eq!(cfg.style.inactive.bg, "#242933");
    assert_eq!(
        cfg.style.inactive.color_mode,
        mpx::config::InactiveColorMode::Desaturate
    );
    assert_eq!(cfg.style.inactive.factor, Some(0.35));
    assert_eq!(cfg.style.inactive.dim_content_bg, Some(false));
}

#[test]
fn allow_passthrough_defaults_to_true_and_can_be_disabled() {
    assert!(Config::default().allow_passthrough);
    let cfg = Config::parse("allow_passthrough = false\n").unwrap();
    assert!(!cfg.allow_passthrough);
}

#[test]
fn agents_get_mpx_tools_unless_turned_off() {
    assert!(Config::default().agent.mcp);
    let cfg = Config::parse("[agent]\nmcp = false\n").unwrap();
    assert!(!cfg.agent.mcp);
}
