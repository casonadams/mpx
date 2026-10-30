//! tmux shim: command parsing, command strings, option values and server selection.
use std::path::PathBuf;

use mpx::config::Config;
use mpx::tmux_shim::{
    buffer_sample, builtin_option, config_lines, default_session, parse_invocation,
    quote_option_value, server_key, split_commands, split_direction,
};
use mpx::layout::Direction;

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn words(line: &str) -> Vec<Vec<String>> {
    split_commands(line).unwrap()
}

#[test]
fn command_strings_split_on_unescaped_semicolons_and_respect_quotes() {
    assert_eq!(
        words("set -g status off ; split-window -h"),
        [vec!["set", "-g", "status", "off"], vec!["split-window", "-h"]]
    );
    assert_eq!(
        words("display 'a ; b' \"c d\" e\\;f"),
        [vec!["display", "a ; b", "c d", "e;f"]]
    );
    assert_eq!(words("send-keys \"say \\\"hi\\\"\";"), [vec!["send-keys", "say \"hi\""]]);
    assert!(words("  ;; ").is_empty());
    assert_eq!(words("a '' b"), [vec!["a", "", "b"]]);
    assert!(split_commands("echo 'open").is_err());
    assert!(split_commands("echo \"open").is_err());
}

#[test]
fn config_files_drop_comments_and_join_continuations_keeping_line_numbers() {
    let text = "# comment\n\nset -g a b\nbind x \\\n  kill-pane\n  # indented comment\nlast";
    assert_eq!(
        config_lines(text),
        [
            (3, "set -g a b".to_string()),
            (4, "bind x   kill-pane".to_string()),
            (7, "last".to_string()),
        ]
    );
}

#[test]
fn new_commands_parse_with_their_flags_and_aliases() {
    let inv = parse_invocation(&argv(&["set", "-gq", "@x", "a b"])).unwrap();
    assert_eq!(inv.command, "set-option");
    assert!(inv.has('g') && inv.has('q'));
    assert_eq!(inv.operands, ["@x", "a b"]);

    let inv = parse_invocation(&argv(&["pasteb", "-d", "-b", "mine", "-t", "%2"])).unwrap();
    assert_eq!(inv.command, "paste-buffer");
    assert!(inv.has('d'));
    assert_eq!(inv.value('b'), Some("mine"));
    assert_eq!(inv.value('t'), Some("%2"));

    let inv = parse_invocation(&argv(&["wait", "-S", "done"])).unwrap();
    assert_eq!((inv.command.as_str(), inv.has('S')), ("wait-for", true));
    assert_eq!(inv.operands, ["done"]);

    assert_eq!(parse_invocation(&argv(&["next"])).unwrap().command, "next-window");
    assert_eq!(parse_invocation(&argv(&["prev"])).unwrap().command, "previous-window");
    assert_eq!(parse_invocation(&argv(&["movew", "-t", "2"])).unwrap().command, "move-window");
    assert!(parse_invocation(&argv(&["select-window", "-n"])).unwrap().has('n'));
}

#[test]
fn server_flags_are_recorded_on_the_invocation() {
    let inv = parse_invocation(&argv(&["-L", "work", "-S", "/tmp/x", "ls"])).unwrap();
    assert_eq!(inv.socket_name.as_deref(), Some("work"));
    assert_eq!(inv.socket_path.as_deref(), Some("/tmp/x"));
    assert_eq!(inv.command, "list-sessions");
}

#[test]
fn servers_get_their_own_directory_and_the_default_server_has_none() {
    assert_eq!(server_key(None, None), None);
    assert_eq!(server_key(Some("default"), None), None);
    let named = server_key(Some("work"), None).unwrap();
    assert_eq!(named, PathBuf::from("servers/name/work"));
    let by_path = server_key(None, Some("/tmp/a b.sock")).unwrap();
    assert!(by_path.starts_with("servers/path"), "{by_path:?}");
    assert!(!by_path.to_string_lossy().contains(' '));
    assert_eq!(server_key(Some("work"), Some("/p")).unwrap().components().nth(1).unwrap().as_os_str(), "path");
    assert_ne!(server_key(Some("a/b"), None), server_key(Some("a_b"), None));
}

#[test]
fn a_server_without_target_uses_the_lowest_numbered_session_then_names() {
    let live = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(default_session(&[]), "0");
    assert_eq!(default_session(&live(&["work", "10", "2"])), "2");
    assert_eq!(default_session(&live(&["zeta", "alpha"])), "alpha");
}

#[test]
fn split_window_stacks_unless_asked_for_side_by_side() {
    let dir = |args: &[&str]| split_direction(&parse_invocation(&argv(args)).unwrap());
    assert_eq!(dir(&["split-window"]), Direction::Vertical);
    assert_eq!(dir(&["split-window", "-v"]), Direction::Vertical);
    assert_eq!(dir(&["split-window", "-h"]), Direction::Horizontal);
}

#[test]
fn option_values_print_bare_or_quoted_like_show_options() {
    assert_eq!(quote_option_value("on"), "on");
    assert_eq!(quote_option_value(""), "\"\"");
    assert_eq!(quote_option_value("a b"), "\"a b\"");
    assert_eq!(quote_option_value("say \"hi\" $x"), "\"say \\\"hi\\\" \\$x\"");
    assert_eq!(quote_option_value("#[fg=red]"), "\"#[fg=red]\"");
}

#[test]
fn builtin_options_come_from_the_config_and_unknown_ones_are_absent() {
    let config = Config {
        prefix: "C-a".into(),
        remain_on_exit: true,
        ..Config::default()
    };
    assert_eq!(builtin_option("prefix", &config).as_deref(), Some("C-a"));
    assert_eq!(builtin_option("remain-on-exit", &config).as_deref(), Some("on"));
    assert_eq!(builtin_option("base-index", &config).as_deref(), Some("0"));
    assert_eq!(builtin_option("mouse", &config), None);
}

#[test]
fn buffer_samples_escape_control_characters_and_truncate() {
    assert_eq!(buffer_sample("a\nb\tc"), "a\\nb\\tc");
    let long = "x".repeat(80);
    assert_eq!(buffer_sample(&long), format!("{}...", "x".repeat(50)));
    assert_eq!(buffer_sample(&"y".repeat(50)), "y".repeat(50));
}
