use clap::{CommandFactory, Parser};
use mpx::cli::{Cli, Commands};

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("mpx").chain(args.iter().copied())).expect("parses")
}

#[test]
fn every_subcommand_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn split_dash_h_means_side_by_side_like_tmux() {
    let cli = parse(&["split", "-h", "-d", "--", "htop"]);
    match cli.command {
        Some(Commands::Split {
            horizontal,
            detach,
            argv,
            ..
        }) => {
            assert!(horizontal);
            assert!(detach);
            assert_eq!(argv, ["htop"]);
        }
        other => panic!("unexpected parse: {other:?}"),
    }
}

#[test]
fn options_after_the_command_belong_to_the_command() {
    let cli = parse(&["new-window", "sh", "-c", "echo hi"]);
    match cli.command {
        Some(Commands::NewWindow { cwd, argv, .. }) => {
            assert_eq!(cwd, None);
            assert_eq!(argv, ["sh", "-c", "echo hi"]);
        }
        other => panic!("unexpected parse: {other:?}"),
    }
}

#[test]
fn options_after_the_command_are_not_split_options() {
    let cli = parse(&["split", "-h", "sh", "-d", "-c", "x"]);
    match cli.command {
        Some(Commands::Split { detach, argv, .. }) => {
            assert!(!detach);
            assert_eq!(argv, ["sh", "-d", "-c", "x"]);
        }
        other => panic!("unexpected parse: {other:?}"),
    }
}

#[test]
fn options_before_the_command_still_apply() {
    let cli = parse(&["run", "-n", "job", "--pty", "sh", "-c", "x"]);
    match cli.command {
        Some(Commands::Run {
            name, pty, argv, ..
        }) => {
            assert_eq!(name.as_deref(), Some("job"));
            assert!(pty);
            assert_eq!(argv, ["sh", "-c", "x"]);
        }
        other => panic!("unexpected parse: {other:?}"),
    }
    let cli = parse(&["new-window", "-c", "/tmp", "sh", "-c", "x"]);
    match cli.command {
        Some(Commands::NewWindow { cwd, argv, .. }) => {
            assert_eq!(cwd.as_deref(), Some("/tmp"));
            assert_eq!(argv, ["sh", "-c", "x"]);
        }
        other => panic!("unexpected parse: {other:?}"),
    }
}

#[test]
fn double_dash_still_ends_the_options() {
    let cli = parse(&["new-window", "--", "sh", "-c", "x"]);
    match cli.command {
        Some(Commands::NewWindow { argv, .. }) => assert_eq!(argv, ["sh", "-c", "x"]),
        other => panic!("unexpected parse: {other:?}"),
    }
}

#[test]
fn run_and_split_require_a_command_but_new_window_does_not() {
    assert!(Cli::try_parse_from(["mpx", "run", "-n", "x"]).is_err());
    assert!(Cli::try_parse_from(["mpx", "split", "-h"]).is_err());
    assert!(Cli::try_parse_from(["mpx", "new-window"]).is_ok());
}
