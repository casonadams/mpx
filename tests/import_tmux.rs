//! `mpx import-tmux` conversion of a tmux config.
use mpx::config::{BorderStyle, Config};
use mpx::import_tmux::{Converted, convert};
use mpx::tui::keymap;

const SAMPLE: &str = r##"# mpx-style tmux.conf used by the import tests
unbind C-b
set -g prefix C-a
bind C-a send-prefix
set -g history-limit 10000
set -g remain-on-exit on
set -g allow-passthrough off
set -g status-style "bg=colour235, fg=colour250"
set -g pane-border-style fg=colour240
set -g pane-active-border-style fg=green
set -g pane-border-lines rounded
set -g mouse on
set -g escape-time 0
set -g base-index 1
set -g @plugin 'tmux-plugins/tpm'

bind | split-window -h -c "#{pane_current_path}"
bind - split-window -v
bind h select-pane -L
bind j select-pane -D
bind k select-pane -U
bind l select-pane -R
bind -r H resize-pane -L
bind -n M-h select-pane -L
bind -T root F12 palette
bind c new-window -c "#{pane_current_path}"
bind r command-prompt -I 'rename-window '
bind x confirm-before -p "kill #W? (y/n)" kill-window
bind b split-window \
  -h
bind s split-window -h ; select-layout even-horizontal
run '~/.tmux/plugins/tpm/tpm'
%if "#{==:#{version},3.2}"
set -g status-position top
%endif
"##;

fn parsed(c: &Converted) -> Config {
    Config::parse(&c.toml).unwrap_or_else(|e| panic!("toml did not parse: {e}\n{}", c.toml))
}

fn has_note(c: &Converted, line: usize) -> bool {
    let prefix = format!("line {line}: ");
    c.notes.iter().any(|n| n.starts_with(&prefix))
}

fn parsed_clean(input: &str) -> Config {
    let c = convert(input);
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    parsed(&c)
}

#[test]
fn status_line_counts_and_status_formats_are_imported() {
    let config = parsed(&convert(
        "set -g status 3\nset -g status-format[1] \"#{session_name}\"\n",
    ));
    assert_eq!((config.status, config.status_lines), (true, 3));
    assert_eq!(config.status_format, ["", "#{session_name}"]);
}

#[test]
fn status_on_off_and_bad_values_are_imported_or_noted() {
    let off = convert("set -g status off\n");
    assert!(!parsed(&off).status);
    let bad = convert("set -g status 9\n");
    assert!(has_note(&bad, 1));
}

#[test]
fn empty_input_converts_to_default_config() {
    let c = convert("");
    assert!(c.notes.is_empty());
    assert_eq!(parsed(&c), Config::default());
}

#[test]
fn comments_and_blank_lines_convert_to_default_config() {
    let c = convert("# nothing here\n\n   # indented comment\n\t\n");
    assert!(c.notes.is_empty());
    assert_eq!(parsed(&c), Config::default());
}

#[test]
fn sample_output_parses_and_maps_settings() {
    let c = convert(SAMPLE);
    let config = parsed(&c);
    assert_eq!(config.prefix, "C-a");
    assert_eq!(config.session.scrollback_lines, 10000);
    assert_eq!(config.remain_on_exit.as_str(), "on");
    assert_eq!(config.allow_passthrough.as_str(), "off");
    assert_eq!(config.style.status_style, "bg=colour235, fg=colour250");
    assert_eq!(config.style.pane_border_style, "fg=colour240");
    assert_eq!(config.style.pane_active_border_style, "fg=green");
    assert_eq!(config.style.border_style, BorderStyle::Rounded);
}

#[test]
fn sample_maps_prefix_and_root_bindings() {
    let config = parsed(&convert(SAMPLE));
    assert_eq!(
        config.bind.get("C-a").map(String::as_str),
        Some("send-prefix")
    );
    assert_eq!(config.bind.get("C-b").map(String::as_str), Some(""));
    assert_eq!(
        config.bind.get("h").map(String::as_str),
        Some("select-pane -L")
    );
    assert_eq!(
        config.bind.get("|").map(String::as_str),
        Some("split-window -h")
    );
    assert_eq!(
        config.bind.get("b").map(String::as_str),
        Some("split-window -h")
    );
    assert_eq!(config.bind.get("c").map(String::as_str), Some("new-window"));
    assert_eq!(
        config.bind.get("r").map(String::as_str),
        Some("command-prompt -I 'rename-window '")
    );
    assert_eq!(
        config.bind.get("s").map(String::as_str),
        Some("split-window -h")
    );
    assert_eq!(
        config.bind.get("H").map(String::as_str),
        Some("resize-pane -L")
    );
    assert_eq!(
        config.bind_root.get("M-h").map(String::as_str),
        Some("select-pane -L")
    );
    assert_eq!(
        config.bind_root.get("F12").map(String::as_str),
        Some("palette")
    );
}

#[test]
fn sample_skips_unsupported_bindings() {
    let config = parsed(&convert(SAMPLE));
    assert!(
        !config.bind.contains_key("-"),
        "split-window -v is not an mpx option"
    );
    assert!(
        config
            .bind
            .get("x")
            .is_some_and(|cmd| cmd.contains("confirm-before")),
        "confirm-before is an mpx command now"
    );
}

#[test]
fn sample_output_resolves_to_a_keymap() {
    let config = parsed(&convert(SAMPLE));
    let keymap = keymap::resolve(&config).expect("converted bindings resolve");
    let pipe = keymap
        .bindings()
        .iter()
        .find(|b| b.key.display() == "|")
        .expect("pipe binding present");
    assert_eq!(pipe.source, "split-window -h");
}

#[test]
fn sample_notes_carry_original_line_numbers() {
    let c = convert(SAMPLE);
    for line in [2, 13, 14, 15, 17, 18, 23, 26, 31, 32, 33] {
        assert!(
            has_note(&c, line),
            "expected a note for line {line}: {:?}",
            c.notes
        );
    }
    for line in [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 19, 20, 21, 22, 24, 25, 27, 28, 34, 35,
    ] {
        assert!(
            !has_note(&c, line),
            "unexpected note for line {line}: {:?}",
            c.notes
        );
    }
}

#[test]
fn sample_notes_name_the_skipped_command() {
    let c = convert(SAMPLE);
    let note = |line: usize| {
        c.notes
            .iter()
            .find(|n| n.starts_with(&format!("line {line}: ")))
            .cloned()
            .unwrap_or_default()
    };
    assert!(note(32).contains("runs at tmux startup"), "{}", note(32));
    assert!(note(33).contains("%if"), "{}", note(33));
    assert!(note(31).contains("'select-layout'"), "{}", note(31));
    assert!(
        convert(SAMPLE).toml.contains("mouse = true"),
        "mouse is a file key now"
    );
}

#[test]
fn wrap_search_imports_as_the_wrap_search_file_key() {
    let c = convert("set -g wrap-search off\n");
    assert!(c.toml.contains("wrap_search = false"), "{}", c.toml);
    assert!(!parsed(&c).wrap_search);
}

#[test]
fn unsupported_lines_are_also_commented_in_toml() {
    let c = convert(SAMPLE);
    assert!(c.toml.contains("# unsupported (line 33): "), "{}", c.toml);
    assert_eq!(
        c.notes.len(),
        c.toml.matches("# unsupported (line ").count()
    );
}

#[test]
fn skipped_conditional_contents_are_not_applied() {
    let config = parsed(&convert(SAMPLE));
    assert_eq!(config.style.border_style, BorderStyle::Rounded);
    assert!(!convert(SAMPLE).toml.contains("status-position"));
}

#[test]
fn continuation_joins_physical_lines() {
    let c = convert("bind b split-window \\\n  -h\n");
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    assert_eq!(
        parsed(&c).bind.get("b").map(String::as_str),
        Some("split-window -h")
    );
}

#[test]
fn semicolon_separates_top_level_commands_and_escaped_semicolon_chains_binds() {
    let config = parsed_clean(
        "set -g prefix C-a; set -g history-limit 5\nbind x split-window \\; select-layout tiled\n",
    );
    assert_eq!(config.prefix, "C-a");
    assert_eq!(config.session.scrollback_lines, 5);
    assert_eq!(
        config.session.history_limit,
        Config::default().session.history_limit
    );
    assert_eq!(
        config.bind.get("x").map(String::as_str),
        Some("split-window ; select-layout tiled")
    );
}

#[test]
fn quoted_words_survive_into_mpx_command_lines() {
    let config = parsed_clean("bind R command-prompt -I \"rename-session \"\n");
    assert_eq!(
        config.bind.get("R").map(String::as_str),
        Some("command-prompt -I 'rename-session '")
    );
}

#[test]
fn quote_characters_in_keys_are_escaped_for_toml() {
    let c = convert("bind '\"' split-window\n");
    let config = parsed(&c);
    assert_eq!(
        config.bind.get("\"").map(String::as_str),
        Some("split-window")
    );
    assert!(c.toml.contains("\"\\\"\" = \"split-window\""), "{}", c.toml);
}

#[test]
fn bind_table_and_root_flag_select_the_mpx_table() {
    let config = parsed(&convert(
        "bind -T root M-j select-pane -D\nbind -n M-k select-pane -U\n",
    ));
    assert_eq!(
        config.bind_root.get("M-j").map(String::as_str),
        Some("select-pane -D")
    );
    assert_eq!(
        config.bind_root.get("M-k").map(String::as_str),
        Some("select-pane -U")
    );
    assert!(config.bind.is_empty());
}

#[test]
fn other_key_tables_are_skipped_with_a_note() {
    let c = convert("bind -T copy-mode-vi v send-keys -X begin-selection\n");
    assert!(parsed(&c).bind.is_empty());
    assert!(has_note(&c, 1), "{:?}", c.notes);
}

#[test]
fn invalid_prefix_and_colours_keep_defaults_and_are_noted() {
    let c = convert("set -g prefix2 C-a\nset -g status-style bg=notacolour\n");
    let config = parsed(&c);
    assert_eq!(config.prefix, Config::default().prefix);
    assert_eq!(config.style.status_bg, Config::default().style.status_bg);
    assert!(has_note(&c, 1) && has_note(&c, 2), "{:?}", c.notes);
}

#[test]
fn crlf_line_endings_are_accepted() {
    let config = parsed(&convert("set -g prefix C-a\r\nset -g history-limit 7\r\n"));
    assert_eq!(config.prefix, "C-a");
    assert_eq!(config.session.scrollback_lines, 7);
}

#[test]
fn options_map_to_their_config_keys() {
    let config = parsed_clean(
        "set -g remain-on-exit failed\nset -g allow-passthrough all\nset -g monitor-bell off\nset -g automatic-rename off\nset -g default-shell /bin/zsh\nset -g history-limit 2000\nset -g focus-events on\nset -g extended-keys always\nset -g xterm-keys off\nset -g set-titles on\nset -g set-titles-string \"#S: #W\"\nset -g bell-action current\nset -g visual-bell on\nset -g visual-activity on\n",
    );
    assert_eq!(config.remain_on_exit.as_str(), "failed");
    assert_eq!(config.allow_passthrough.as_str(), "all");
    assert!(!config.window.monitor_bell && !config.window.automatic_rename);
    assert_eq!(config.session.default_shell.as_deref(), Some("/bin/zsh"));
    assert_eq!(config.session.scrollback_lines, 2000);
    assert!(config.terminal.focus_events);
    assert_eq!(config.terminal.extended_keys.as_str(), "always");
    assert!(!config.terminal.xterm_keys);
    assert!(config.terminal.set_titles);
    assert_eq!(config.terminal.set_titles_string, "#S: #W");
    assert_eq!(config.terminal.bell_action.as_str(), "current");
    assert!(config.terminal.visual_bell && config.terminal.visual_activity);
}

#[test]
fn bad_values_for_terminal_and_bell_options_are_noted_and_skipped() {
    let c = convert(
        "set -g extended-keys maybe\nset -g bell-action loud\nset -g focus-events sometimes\n",
    );
    let config = parsed(&c);
    assert_eq!(config.terminal.extended_keys.as_str(), "off");
    assert_eq!(config.terminal.bell_action.as_str(), "any");
    assert!(!config.terminal.focus_events);
    assert_eq!(c.notes.len(), 3, "{:?}", c.notes);
}

#[test]
fn style_strings_and_monitor_silence_convert() {
    let config = parsed(&convert(
        "set -g status-style bg=colour235,fg=white,bold\nset -g window-status-current-style reverse\nset -g monitor-silence 30\n",
    ));
    assert_eq!(config.style.status_style, "bg=colour235,fg=white,bold");
    assert_eq!(config.style.window_status_current_style, "reverse");
    assert_eq!(config.window.monitor_silence, 30);
}

#[test]
fn refused_scopes_and_style_items_are_noted_per_line() {
    let c = convert(
        "set -g message-style blink\nset -p -g mode-keys vi\nset -s status on\nset -g synchronize-panes on\n",
    );
    assert!((1..=4).all(|line| has_note(&c, line)), "{:?}", c.notes);
}

#[test]
fn command_aliases_become_the_alias_table() {
    let c = convert("set -s command-alias[0] 'sp=split-window -h'\n");
    assert!(c.toml.contains("[alias]"), "{}", c.toml);
    assert!(
        c.toml.contains("\"sp\" = \"split-window -h\""),
        "{}",
        c.toml
    );
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    let config = mpx::config::Config::parse(&c.toml).unwrap();
    assert_eq!(
        config.alias.get("sp").map(String::as_str),
        Some("split-window -h")
    );
}

#[test]
fn if_shell_and_run_shell_binds_convert() {
    let c = convert(
        "bind x if-shell -F '#{pane_active}' 'next-window' 'prev-window'\nbind y run-shell 'echo hi'\n",
    );
    let config = mpx::config::Config::parse(&c.toml).unwrap();
    let binds: Vec<&str> = ["x", "y"]
        .iter()
        .filter_map(|key| config.bind.get(*key))
        .map(String::as_str)
        .collect();
    assert_eq!(
        binds,
        [
            "if-shell -F #{pane_active} next-window prev-window",
            "run-shell 'echo hi'"
        ]
    );
}

#[test]
fn startup_run_shell_has_no_config_equivalent_and_is_noted() {
    let c = convert("run-shell 'echo hi'\n");
    assert!(
        c.notes.iter().any(|n| n.contains("mpx run-shell")),
        "{:?}",
        c.notes
    );
}
