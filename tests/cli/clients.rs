//! `new-session -t` names a session group, which mpx refuses: each session owns its panes in its
//! own daemon, so a group cannot share windows.
use clap::Parser;
use mpx::cli::{Cli, run_cli};

use super::fixture::Fixture;

#[test]
fn new_session_in_a_group_is_refused() {
    let f = Fixture::start();
    let cli = Cli::try_parse_from(f.argv(&["new-session", "-d", "-t", "t"])).expect("parse");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let err = runtime
        .block_on(run_cli(cli))
        .expect_err("a session group is refused");
    assert!(err.contains("session groups are not supported"), "{err}");
    assert!(err.contains("own daemon"), "{err}");
}
