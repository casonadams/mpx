//! Pane scrollback saved next to the snapshot while `restore` is on, and shown again on restore.
use std::path::Path;
use std::time::{Duration, Instant};

use mpx::config::{Config, SessionOptions};
use mpx::store::{PaneScrollback, ScrollbackStore, SnapshotStore};

use super::support::{connect, logs_until, pty, start_on};

/// The value `probe` yields, polled for at most two seconds.
async fn eventually<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn saved_log(state: &Path, pane: &str) -> Option<String> {
    let saved = ScrollbackStore::in_dir(state).load(pane).unwrap()?;
    Some(String::from_utf8_lossy(&saved.log).into_owned())
}

fn restore(on: bool) -> Config {
    Config {
        session: SessionOptions {
            restore: on,
            ..SessionOptions::default()
        },
        ..Config::default()
    }
}

/// Starts a session on `state` whose `sh` pane `shell` has printed `saved-42`, then opens a second
/// pane, so the snapshot is saved.
async fn session_with_output(
    state: &Path,
    config: Config,
) -> (super::support::TestDaemon, mpx::client::Client) {
    let daemon = start_on(state, config).await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("shell", "sh", &[])).await.unwrap();
    client
        .send_input("shell", "echo saved-$((6*7))\r")
        .await
        .unwrap();
    logs_until(&mut client, "shell", "saved-42").await;
    client.spawn(pty("other", "cat", &[])).await.unwrap();
    (daemon, client)
}

#[tokio::test]
async fn output_comes_back_on_restore_after_the_daemon_stopped_without_shutting_down() {
    let state = tempfile::tempdir().unwrap();
    let (_first, _client) = session_with_output(state.path(), Config::default()).await;
    eventually("the saved scrollback", || {
        saved_log(state.path(), "shell").filter(|log| log.contains("saved-42"))
    })
    .await;

    let second = start_on(state.path(), Config::default()).await;
    let mut client = connect(&second.endpoint).await;
    let logs = logs_until(&mut client, "shell", "saved-42").await;
    assert!(logs.contains("echo saved-$((6*7))"), "{logs}");
}

#[tokio::test]
async fn with_restore_off_stale_scrollback_is_deleted() {
    let state = tempfile::tempdir().unwrap();
    let stale = PaneScrollback {
        size: (80, 24),
        log: b"old secret".to_vec(),
    };
    ScrollbackStore::in_dir(state.path())
        .save("stale", &stale)
        .unwrap();

    let (_daemon, _client) = session_with_output(state.path(), restore(false)).await;
    eventually("a snapshot with both panes", || {
        let snapshot = SnapshotStore::in_dir(state.path()).load().unwrap()?;
        let panes: usize = snapshot.windows.iter().map(|w| w.panes.len()).sum();
        (panes == 2).then_some(())
    })
    .await;
    eventually("the stale scrollback to go", || {
        saved_log(state.path(), "stale").is_none().then_some(())
    })
    .await;
    assert_eq!(saved_log(state.path(), "shell"), None);
}

#[tokio::test]
async fn a_killed_panes_scrollback_is_deleted() {
    let state = tempfile::tempdir().unwrap();
    let (_daemon, mut client) = session_with_output(state.path(), restore(true)).await;
    eventually("the saved scrollback", || saved_log(state.path(), "shell")).await;

    client.kill("shell").await.unwrap();

    eventually("the scrollback to go", || {
        saved_log(state.path(), "shell").is_none().then_some(())
    })
    .await;
}

#[tokio::test]
async fn an_unreadable_snapshot_is_moved_aside_and_its_scrollback_kept() {
    let state = tempfile::tempdir().unwrap();
    std::fs::write(state.path().join("snapshot.json"), "{ not json").unwrap();
    ScrollbackStore::in_dir(state.path())
        .save(
            "kept",
            &PaneScrollback {
                size: (80, 24),
                log: b"old output".to_vec(),
            },
        )
        .unwrap();
    let daemon = super::support::start_on(state.path(), restore(true)).await;
    let mut client = super::support::connect(&daemon.endpoint).await;
    client
        .new_window(super::support::window("w", "pw"))
        .await
        .unwrap();

    eventually("a fresh snapshot", || {
        SnapshotStore::in_dir(state.path()).load().ok()?.map(|_| ())
    })
    .await;
    let moved = std::fs::read_dir(state.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("snapshot.json.corrupt-")
    });
    assert!(moved, "the unreadable snapshot is kept aside");
    assert!(
        saved_log(state.path(), "kept").is_some(),
        "its scrollback is not pruned"
    );
}
