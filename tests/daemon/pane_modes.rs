//! Pane modes (copy mode) as the attached terminals report them: a pane is in a mode while any
//! terminal has it in one, a terminal's marks go when it disconnects, and a pane that leaves the
//! session takes its mode with it.
use std::time::{Duration, Instant};

use super::support::{connect, start, task};
use mpx::client::Client;

/// Polls the daemon's pane modes until they equal `want` (bounded).
async fn modes_become(client: &mut Client, want: &[&str]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let modes = client.pane_modes().await.expect("pane modes");
        if modes == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "pane modes {modes:?}, want {want:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_pane_stays_in_mode_until_every_terminal_that_set_it_leaves() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("p", "sleep", &["30"])).await.unwrap();
    let mut first = connect(&daemon.endpoint).await;
    let mut second = connect(&daemon.endpoint).await;

    first.set_pane_mode("p", true, 1).await.unwrap();
    second.set_pane_mode("p", true, 2).await.unwrap();
    assert_eq!(client.pane_modes().await.unwrap(), ["p"]);

    first.set_pane_mode("p", false, 1).await.unwrap();
    assert_eq!(
        client.pane_modes().await.unwrap(),
        ["p"],
        "the second terminal is still in it"
    );

    second.set_pane_mode("p", false, 2).await.unwrap();
    assert!(client.pane_modes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_terminal_that_disconnects_takes_its_pane_modes_with_it() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("p", "sleep", &["30"])).await.unwrap();
    client.spawn(task("q", "sleep", &["30"])).await.unwrap();
    let mut gone = connect(&daemon.endpoint).await;
    gone.set_pane_mode("p", true, 7).await.unwrap();
    gone.set_pane_mode("q", true, 7).await.unwrap();
    let mut stays = connect(&daemon.endpoint).await;
    stays.set_pane_mode("q", true, 8).await.unwrap();
    assert_eq!(client.pane_modes().await.unwrap(), ["p", "q"]);

    drop(gone);
    modes_become(&mut client, &["q"]).await;
    drop(stays);
    modes_become(&mut client, &[]).await;
}

#[tokio::test]
async fn a_pane_that_leaves_the_session_is_out_of_mode_and_a_new_pane_of_its_name_starts_out_of_it()
{
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(task("p", "sleep", &["30"])).await.unwrap();
    let mut terminal = connect(&daemon.endpoint).await;
    terminal.set_pane_mode("p", true, 1).await.unwrap();
    assert_eq!(client.pane_modes().await.unwrap(), ["p"]);

    client.kill("p").await.unwrap();
    modes_become(&mut client, &[]).await;

    client.spawn(task("p", "sleep", &["30"])).await.unwrap();
    assert!(client.pane_modes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_client_command_settles_only_once_the_terminal_reports_it_applied() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let (term_tx, _term_rx) = tokio::sync::mpsc::channel(64);
    let terminal = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = terminal
            .stream_logs_into((None, None), true, term_tx, |e| e)
            .await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let seq = client.client_command("detach-client").await.unwrap();
    let mut waiter = connect(&daemon.endpoint).await;
    let settled = tokio::spawn(async move { waiter.wait_command(seq).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !settled.is_finished(),
        "the terminal has not applied the command yet"
    );

    client.command_applied(seq).await.unwrap();
    let done = tokio::time::timeout(Duration::from_secs(1), settled).await;
    assert!(matches!(done, Ok(Ok(Ok(())))), "{done:?}");
}
