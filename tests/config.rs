use mpx::config::{Config, Passthrough, RemainOnExit};

#[test]
fn partial_file_keeps_defaults_for_missing_keys() {
    let cfg = Config::parse("prefix = \"C-a\"\nhistory_limit = 128\n").unwrap();
    assert_eq!(cfg.prefix, "C-a");
    assert_eq!(cfg.history_limit, 128);
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
fn allow_passthrough_defaults_to_on_and_can_be_disabled() {
    assert_eq!(Config::default().allow_passthrough, Passthrough::On);
    let cfg = Config::parse("allow_passthrough = false\n").unwrap();
    assert_eq!(cfg.allow_passthrough, Passthrough::Off);
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

#[test]
fn remain_on_exit_decides_by_exit_status() {
    assert!(RemainOnExit::On.keeps(Some(0)) && RemainOnExit::On.keeps(Some(3)));
    assert!(RemainOnExit::On.keeps(None));
    assert!(!RemainOnExit::Off.keeps(Some(3)));
    assert!(!RemainOnExit::Failed.keeps(Some(0)));
    assert!(RemainOnExit::Failed.keeps(Some(3)));
    assert!(RemainOnExit::Failed.keeps(None), "a signal is a failure");
}

#[test]
fn file_keys_accept_the_old_switches_and_the_new_words() {
    let mode = |text: &str| Config::parse(text).map(|c| c.remain_on_exit);
    assert_eq!(mode("remain_on_exit = true"), Ok(RemainOnExit::On));
    assert_eq!(mode("remain_on_exit = false"), Ok(RemainOnExit::Off));
    assert_eq!(mode("remain_on_exit = \"failed\""), Ok(RemainOnExit::Failed));
    assert!(mode("remain_on_exit = \"maybe\"").is_err());
    assert_eq!(
        Config::parse("allow_passthrough = \"all\"").unwrap().allow_passthrough,
        Passthrough::All
    );
}

#[test]
fn passthrough_follows_its_scope() {
    assert!(!Passthrough::Off.allows(true));
    assert!(Passthrough::On.allows(true));
    assert!(!Passthrough::On.allows(false));
    assert!(Passthrough::All.allows(false));
}

#[test]
fn defaults_keep_the_status_line_mpx_has_always_drawn() {
    let config = Config::default();
    assert_eq!((config.base_index, config.pane_base_index), (0, 0));
    assert!(config.mouse && config.status && config.set_clipboard);
    assert_eq!(config.status_position, mpx::config::StatusPosition::Bottom);
    assert_eq!(config.status_justify, mpx::config::Justify::Left);
    assert_eq!(config.mode_keys, mpx::config::ModeKeys::Vi);
    assert_eq!(config.status_left, None);
    assert_eq!(config.status_right, None);
    assert_eq!(config.display_time_ms, 3000);
}

#[test]
fn ui_options_read_from_the_config_file() {
    let config = Config::parse(
        "mouse = false\nmode_keys = \"emacs\"\nstatus_position = \"top\"\n\
         status_justify = \"centre\"\nbase_index = 1\nstatus_left = \"[a]\"\n",
    )
    .unwrap();
    assert!(!config.mouse);
    assert_eq!(config.mode_keys, mpx::config::ModeKeys::Emacs);
    assert_eq!(config.status_position, mpx::config::StatusPosition::Top);
    assert_eq!(config.status_justify, mpx::config::Justify::Centre);
    assert_eq!(config.base_index, 1);
    assert_eq!(config.status_left.as_deref(), Some("[a]"));
}
