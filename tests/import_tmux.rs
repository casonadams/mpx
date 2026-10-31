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
    assert_eq!(config.scrollback_lines, 10000);
    assert_eq!(config.remain_on_exit.as_str(), "on");
    assert_eq!(config.allow_passthrough.as_str(), "off");
    assert_eq!(config.style.status_bg, "colour235");
    assert_eq!(config.style.status_fg, "colour250");
    assert_eq!(config.style.border_inactive, "colour240");
    assert_eq!(config.style.border_active, "green");
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
    assert!(
        note(32).contains("plugin/shell commands are not run"),
        "{}",
        note(32)
    );
    assert!(note(33).contains("%if"), "{}", note(33));
    assert!(note(31).contains("'select-layout'"), "{}", note(31));
    assert!(convert(SAMPLE).toml.contains("mouse = true"), "mouse is a file key now");
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
    let c = convert(
        "set -g prefix C-a; set -g history-limit 5\nbind x split-window \\; select-layout tiled\n",
    );
    let config = parsed(&c);
    assert_eq!(config.prefix, "C-a");
    assert_eq!(config.scrollback_lines, 5);
    assert_eq!(config.history_limit, Config::default().history_limit);
    assert_eq!(
        config.bind.get("x").map(String::as_str),
        Some("split-window ; select-layout tiled")
    );
    assert!(c.notes.is_empty(), "{:?}", c.notes);
}

#[test]
fn quoted_words_survive_into_mpx_command_lines() {
    let c = convert("bind R command-prompt -I \"rename-session \"\n");
    let config = parsed(&c);
    assert_eq!(
        config.bind.get("R").map(String::as_str),
        Some("command-prompt -I 'rename-session '")
    );
    assert!(c.notes.is_empty(), "{:?}", c.notes);
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
    assert_eq!(config.scrollback_lines, 7);
}

#[test]
fn new_options_map_to_their_config_keys() {
    let c = convert(
        "set -g remain-on-exit failed\nset -g allow-passthrough all\nset -g monitor-bell off\nset -g automatic-rename off\nset -g default-shell /bin/zsh\nset -g history-limit 2000\n",
    );
    let config = parsed(&c);
    assert_eq!(config.remain_on_exit.as_str(), "failed");
    assert_eq!(config.allow_passthrough.as_str(), "all");
    assert!(!config.monitor_bell && !config.automatic_rename);
    assert_eq!(config.default_shell.as_deref(), Some("/bin/zsh"));
    assert_eq!(config.scrollback_lines, 2000);
    assert!(c.notes.is_empty(), "{:?}", c.notes);
}
