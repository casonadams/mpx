use mpx::config::{Config, Passthrough, RemainOnExit};

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
fn allow_passthrough_defaults_to_on_and_can_be_disabled() {
    assert_eq!(Config::default().allow_passthrough, Passthrough::On);
    let cfg = Config::parse("allow_passthrough = false\n").unwrap();
    assert_eq!(cfg.allow_passthrough, Passthrough::Off);
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

fn applied(name: &str, value: &str) -> Result<Config, String> {
    let mut config = Config::default();
    config.apply_option(name, value)?;
    Ok(config)
}

#[test]
fn remain_on_exit_takes_on_off_or_failed() {
    let mode = |v| applied("remain-on-exit", v).map(|c| c.remain_on_exit);
    assert_eq!(mode("on"), Ok(RemainOnExit::On));
    assert_eq!(mode("off"), Ok(RemainOnExit::Off));
    assert_eq!(mode("failed"), Ok(RemainOnExit::Failed));
    assert_eq!(mode("maybe"), Err("invalid value: maybe".to_string()));
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
fn history_limit_sets_pane_scrollback_and_not_exited_node_history() {
    assert_eq!(Config::default().scrollback_lines, 100_000);
    let config = applied("history-limit", "2000").unwrap();
    assert_eq!(config.scrollback_lines, 2000);
    assert_eq!(config.history_limit, Config::default().history_limit);
    assert!(applied("history-limit", "many").is_err());
}

#[test]
fn allow_passthrough_takes_on_off_or_all() {
    let mode = |v| applied("allow-passthrough", v).map(|c| c.allow_passthrough);
    assert_eq!(mode("on"), Ok(Passthrough::On));
    assert_eq!(mode("off"), Ok(Passthrough::Off));
    assert_eq!(mode("all"), Ok(Passthrough::All));
}

#[test]
fn passthrough_follows_its_scope() {
    assert!(!Passthrough::Off.allows(true));
    assert!(Passthrough::On.allows(true));
    assert!(!Passthrough::On.allows(false));
    assert!(Passthrough::All.allows(false));
}

#[test]
fn automatic_rename_and_monitors_default_on_and_switch_off() {
    let defaults = Config::default();
    assert!(defaults.automatic_rename && defaults.monitor_activity && defaults.monitor_bell);
    assert!(!applied("automatic-rename", "off").unwrap().automatic_rename);
    assert!(!applied("monitor-activity", "off").unwrap().monitor_activity);
    assert!(!applied("monitor-bell", "off").unwrap().monitor_bell);
    assert!(applied("monitor-activity", "maybe").is_err());
}

#[test]
fn monitor_silence_is_accepted_only_as_off() {
    assert!(applied("monitor-silence", "0").is_ok());
    assert_eq!(
        applied("monitor-silence", "5").map(|_| ()),
        Err("monitor-silence is not supported by mpx".to_string())
    );
}

#[test]
fn default_terminal_shell_and_command_are_validated_and_stored() {
    let defaults = Config::default();
    assert_eq!(defaults.default_terminal, "xterm-256color");
    assert_eq!(defaults.default_shell, None);
    assert_eq!(defaults.default_command, None);
    let term = applied("default-terminal", "screen-256color").unwrap();
    assert_eq!(term.default_terminal, "screen-256color");
    assert!(applied("default-terminal", " ").is_err());
    let shell = applied("default-shell", "/bin/zsh").unwrap();
    assert_eq!(shell.default_shell.as_deref(), Some("/bin/zsh"));
    assert!(applied("default-shell", "zsh").is_err(), "must be an absolute path");
    let command = applied("default-command", "htop").unwrap();
    assert_eq!(command.default_command.as_deref(), Some("htop"));
    assert_eq!(applied("default-command", "").unwrap().default_command, None);
}

#[test]
fn display_panes_time_sets_the_numbers_timeout() {
    assert_eq!(applied("display-panes-time", "2500").unwrap().display_panes_time_ms, 2500);
    assert!(applied("display-panes-time", "soon").is_err());
}

#[test]
fn options_without_an_mpx_equivalent_are_not_applied() {
    let mut config = Config::default();
    assert_eq!(config.apply_option("status-keys", "vi"), Ok(false));
    assert_eq!(config, Config::default());
}

fn set(config: &mut Config, name: &str, value: &str) -> Result<bool, String> {
    config.apply_option(name, value)
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
fn numbering_mouse_status_and_clipboard_options_set_their_fields() {
    let mut config = Config::default();
    assert_eq!(set(&mut config, "base-index", "1"), Ok(true));
    assert_eq!(set(&mut config, "pane-base-index", "1"), Ok(true));
    assert_eq!(set(&mut config, "mouse", "off"), Ok(true));
    assert_eq!(set(&mut config, "status", "off"), Ok(true));
    assert_eq!(set(&mut config, "status-position", "top"), Ok(true));
    assert_eq!(set(&mut config, "status-interval", "5"), Ok(true));
    assert_eq!(set(&mut config, "status-left", "[#{session_name}]"), Ok(true));
    assert_eq!(set(&mut config, "status-right", "%H:%M"), Ok(true));
    assert_eq!(set(&mut config, "status-left-length", "4"), Ok(true));
    assert_eq!(set(&mut config, "status-right-length", "8"), Ok(true));
    assert_eq!(set(&mut config, "window-status-format", "#I"), Ok(true));
    assert_eq!(set(&mut config, "window-status-current-format", "#I*"), Ok(true));
    assert_eq!(set(&mut config, "window-status-separator", " | "), Ok(true));
    assert_eq!(set(&mut config, "status-justify", "centre"), Ok(true));
    assert_eq!(set(&mut config, "display-time", "1500"), Ok(true));
    assert_eq!(set(&mut config, "mode-keys", "emacs"), Ok(true));
    assert_eq!(set(&mut config, "set-clipboard", "off"), Ok(true));
    assert_eq!(config.base_index, 1);
    assert_eq!(config.pane_base_index, 1);
    assert!(!config.mouse && !config.status);
    assert_eq!(config.status_position, mpx::config::StatusPosition::Top);
    assert_eq!(config.status_interval, 5);
    assert_eq!(config.status_left.as_deref(), Some("[#{session_name}]"));
    assert_eq!(config.status_right.as_deref(), Some("%H:%M"));
    assert_eq!((config.status_left_length, config.status_right_length), (4, 8));
    assert_eq!(config.window_status_format.as_deref(), Some("#I"));
    assert_eq!(config.window_status_current_format.as_deref(), Some("#I*"));
    assert_eq!(config.window_status_separator.as_deref(), Some(" | "));
    assert_eq!(config.status_justify, mpx::config::Justify::Centre);
    assert_eq!(config.display_time_ms, 1500);
    assert_eq!(config.mode_keys, mpx::config::ModeKeys::Emacs);
    assert!(!config.set_clipboard);
}

#[test]
fn set_clipboard_external_is_the_same_as_on() {
    let mut config = Config::default();
    set(&mut config, "set-clipboard", "off").unwrap();
    assert_eq!(set(&mut config, "set-clipboard", "external"), Ok(true));
    assert!(config.set_clipboard);
}

#[test]
fn renumber_windows_on_is_accepted_and_off_is_refused_with_the_reason() {
    let mut config = Config::default();
    assert_eq!(set(&mut config, "renumber-windows", "on"), Ok(true));
    assert_eq!(
        set(&mut config, "renumber-windows", "off"),
        Err("renumber-windows off is not supported: windows are always renumbered".to_string())
    );
}

#[test]
fn style_options_set_the_status_and_border_colours() {
    let mut config = Config::default();
    assert_eq!(set(&mut config, "status-style", "bg=green,fg=black"), Ok(true));
    assert_eq!(config.style.status_bg, "green");
    assert_eq!(config.style.status_fg, "black");
    assert_eq!(set(&mut config, "pane-border-style", "fg=red"), Ok(true));
    assert_eq!(config.style.border_inactive, "red");
    assert_eq!(set(&mut config, "pane-active-border-style", "fg=#112233"), Ok(true));
    assert_eq!(config.style.border_active, "#112233");
    assert_eq!(set(&mut config, "status-bg", "blue"), Ok(true));
    assert_eq!(config.style.status_bg, "blue");
}

#[test]
fn style_items_mpx_cannot_show_and_unknown_colours_are_refused() {
    let mut config = Config::default();
    assert!(set(&mut config, "status-style", "bold").is_err());
    assert!(set(&mut config, "status-style", "fg=nope").is_err());
    assert!(set(&mut config, "pane-border-style", "bg=red").is_err());
    assert!(set(&mut config, "message-style", "fg=red").is_err());
}

#[test]
fn malformed_ui_values_are_refused() {
    let mut config = Config::default();
    assert!(set(&mut config, "status", "3").is_err(), "multi-line status");
    assert!(set(&mut config, "status-position", "middle").is_err());
    assert!(set(&mut config, "base-index", "x").is_err());
    assert!(set(&mut config, "status-justify", "sideways").is_err());
    assert!(set(&mut config, "mode-keys", "nano").is_err());
    assert!(set(&mut config, "mouse", "maybe").is_err());
    assert_eq!(config, Config::default(), "a refused value changes nothing");
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

#[test]
fn message_style_is_refused_as_unsupported_rather_than_ignored() {
    let mut config = Config::default();
    let err = set(&mut config, "message-style", "bg=red").unwrap_err();
    assert!(err.contains("message-style is not supported"), "{err}");
}
