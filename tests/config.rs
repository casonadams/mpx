use mpx::config::Config;

#[test]
fn partial_file_keeps_defaults_for_missing_keys() {
    let cfg = Config::parse("prefix = \"C-a\"\nhistory_limit = 128\n").unwrap();
    assert_eq!(cfg.prefix, "C-a");
    assert_eq!(cfg.history_limit, 128);
    assert!(cfg.tmux_compat);
}

#[test]
fn unknown_keys_are_rejected() {
    assert!(Config::parse("prefx = \"C-a\"").is_err());
}

#[test]
fn bindings_and_resume_rules_parse() {
    let text = r#"
[bind]
"M-h" = "select-pane -L"

[[resume]]
name = "codex"
detect = ["codex"]
resume = ["codex", "resume", "{id}"]
fallback = ["codex", "resume", "--last"]
"#;
    let cfg = Config::parse(text).unwrap();
    assert_eq!(
        cfg.bind.get("M-h").map(String::as_str),
        Some("select-pane -L")
    );
    assert_eq!(cfg.resume[0].resume, vec!["codex", "resume", "{id}"]);
    assert_eq!(cfg.resume[0].fallback, vec!["codex", "resume", "--last"]);
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
"##;
    let cfg = Config::parse(text).unwrap();
    assert_eq!(cfg.style.active.fg, "#d8dee9");
    assert_eq!(cfg.style.active.bg, "#2e3440");
    assert_eq!(cfg.style.inactive.fg, "#4c566a");
    assert_eq!(cfg.style.inactive.bg, "#242933");
}

#[test]
fn allow_passthrough_defaults_to_true_and_can_be_disabled() {
    assert!(Config::default().allow_passthrough);
    let cfg = Config::parse("allow_passthrough = false\n").unwrap();
    assert!(!cfg.allow_passthrough);
}

#[test]
fn tmux_compat_defaults_to_true_and_can_be_disabled() {
    assert!(Config::default().tmux_compat);
    let cfg = Config::parse("tmux_compat = false\n").unwrap();
    assert!(!cfg.tmux_compat);
}

#[test]
fn removed_dimming_options_are_rejected_instead_of_ignored() {
    for text in [
        "[style.inactive]\ncolor_mode = \"dim\"\n",
        "[style.inactive]\nfactor = 0.3\n",
        "[style]\ndim_content_bg = true\n",
    ] {
        assert!(Config::parse(text).is_err(), "{text}");
    }
}
