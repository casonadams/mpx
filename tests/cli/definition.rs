use clap::{CommandFactory, Parser};
use mpx::cli::{Cli, Commands};

#[test]
fn every_subcommand_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn split_dash_h_means_side_by_side_like_tmux() {
    let cli = Cli::try_parse_from(["mpx", "split", "-h", "-d", "--", "htop"]).unwrap();
    match cli.command {
        Some(Commands::Split {
            horizontal,
            detach,
            command,
            ..
        }) => {
            assert!(horizontal);
            assert!(detach);
            assert_eq!(command, "htop");
        }
        other => panic!("unexpected parse: {other:?}"),
    }
}
