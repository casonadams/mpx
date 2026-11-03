use std::collections::BTreeMap;

use mpx::protocol::ChannelOp;
use mpx::tui::command::{Command, PaneCommand, ScriptCommand, WindowCommand, parse, parse_in};

fn aliases(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn an_alias_stands_for_its_command_line_with_the_typed_arguments() {
    let table = aliases(&[("sh", "split-window -h")]);
    let cmd = parse_in("sh -d", &table).unwrap();
    assert!(matches!(
        cmd,
        Command::Pane(PaneCommand::SplitWindow { side_by_side: true, detach: true, .. })
    ));
}

#[test]
fn without_the_alias_the_name_is_unknown() {
    assert!(parse("sh").unwrap_err().contains("unknown command"));
}

#[test]
fn an_alias_is_expanded_once_and_applies_inside_sequences() {
    let table = aliases(&[("nxt", "next-window")]);
    let cmd = parse_in("nxt ; nxt", &table).unwrap();
    assert_eq!(
        cmd,
        Command::Sequence(vec![
            Command::Window(WindowCommand::NextWindow),
            Command::Window(WindowCommand::NextWindow),
        ])
    );
}

#[test]
fn set_s_command_alias_sets_and_unsets_an_alias() {
    assert_eq!(
        parse("set -s command-alias[3] sp=split-window").unwrap(),
        Command::Script(ScriptCommand::SetAlias {
            name: "sp".into(),
            command: Some("split-window".into()),
        })
    );
    assert_eq!(
        parse("set -su command-alias sp").unwrap(),
        Command::Script(ScriptCommand::SetAlias {
            name: "sp".into(),
            command: None,
        })
    );
    assert!(parse("set -s command-alias nothing").is_err());
}

#[test]
fn run_shell_takes_flags_then_the_command_words() {
    let cmd = parse("run-shell -b -t %3 -d 0.5 echo hi there").unwrap();
    assert_eq!(
        cmd,
        Command::Script(ScriptCommand::RunShell {
            background: true,
            target: Some("%3".into()),
            delay_ms: Some(500),
            command: "echo hi there".into(),
        })
    );
    assert!(parse("run-shell").unwrap_err().contains("need a shell command"));
    assert!(parse("run-shell -d soon x").unwrap_err().contains("invalid delay"));
}

#[test]
fn if_shell_parses_both_branches_with_aliases() {
    let table = aliases(&[("nxt", "next-window")]);
    let cmd = parse_in("if-shell -F -t %1 '#{pane_active}' 'nxt' 'prev-window'", &table).unwrap();
    assert_eq!(
        cmd,
        Command::Script(ScriptCommand::IfShell {
            background: false,
            format: true,
            target: Some("%1".into()),
            condition: "#{pane_active}".into(),
            then: Box::new(Command::Window(WindowCommand::NextWindow)),
            otherwise: Some(Box::new(Command::Window(WindowCommand::PrevWindow))),
        })
    );
}

#[test]
fn if_shell_needs_a_condition_and_one_or_two_commands() {
    assert!(parse("if-shell true").unwrap_err().contains("need a condition"));
    assert!(parse("if-shell true a b c").is_err());
}

#[test]
fn if_shell_refuses_a_branch_that_does_not_parse() {
    assert!(parse("if-shell true nosuchcommand").unwrap_err().contains("unknown command"));
}

#[test]
fn wait_for_maps_its_flags_to_channel_operations() {
    let ops = [
        ("wait-for ch", ChannelOp::Wait),
        ("wait-for -S ch", ChannelOp::Signal),
        ("wait-for -L ch", ChannelOp::Lock),
        ("wait-for -U ch", ChannelOp::Unlock),
    ];
    for (line, op) in ops {
        assert_eq!(
            parse(line).unwrap(),
            Command::Script(ScriptCommand::WaitFor {
                op,
                channel: "ch".into(),
            }),
            "{line}"
        );
    }
    assert!(parse("wait-for -S -L ch").unwrap_err().contains("one of"));
    assert!(parse("wait-for").unwrap_err().contains("need a channel"));
}

#[test]
fn set_hook_parses_add_remove_and_run_forms() {
    assert_eq!(
        parse("set-hook -a after-new-window 'run-shell hi'").unwrap(),
        Command::Script(ScriptCommand::SetHook {
            name: "after-new-window".into(),
            command: Some("run-shell hi".into()),
            append: true,
        })
    );
    assert_eq!(
        parse("set-hook -u pane-exited").unwrap(),
        Command::Script(ScriptCommand::SetHook {
            name: "pane-exited".into(),
            command: None,
            append: false,
        })
    );
    assert_eq!(
        parse("set-hook -R alert-bell").unwrap(),
        Command::Script(ScriptCommand::RunHook {
            name: "alert-bell".into(),
        })
    );
    assert_eq!(parse("show-hooks").unwrap(), Command::Script(ScriptCommand::ShowHooks));
}

/// Asserts that `line` does not parse, with an error containing `needle`.
fn refused(line: &str, needle: &str) {
    let err = parse(line).expect_err("should be refused");
    assert!(err.contains(needle), "{err}");
}

#[test]
fn set_hook_refuses_unknown_hooks_window_scope_and_missing_commands() {
    refused("set-hook nope 'run-shell x'", "unknown hook");
    refused("set-hook -w after-new-window 'run-shell x'", "not supported");
    refused("set-hook -t %1 pane-exited 'run-shell x'", "not supported");
    refused("set-hook pane-exited", "need a command");
    refused("set-hook pane-exited 'nosuch'", "unknown command");
}

#[test]
fn source_file_takes_paths_and_quiet_and_plain_source_file_reloads() {
    assert_eq!(parse("source-file").unwrap(), Command::ReloadConfig);
    assert_eq!(
        parse("source-file -q a.conf b.conf").unwrap(),
        Command::Script(ScriptCommand::SourceFile {
            paths: vec!["a.conf".into(), "b.conf".into()],
            quiet: true,
        })
    );
}
