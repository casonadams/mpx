//! The session environment: variables set, unset or removed, as later panes see them.
use mpx::protocol::{EnvChange, EnvEntry};

use super::support::{fresh, logs_until, pty, task};

#[tokio::test]
async fn panes_started_after_set_environment_see_the_variable_and_show_it() {
    let (_daemon, mut client) = fresh().await;
    client
        .set_environment("MPX_TEST_GREETING", EnvChange::Set("hello there".into()))
        .await
        .unwrap();
    client
        .spawn(task("t", "sh", &["-c", "echo \"[$MPX_TEST_GREETING]\""]))
        .await
        .unwrap();
    logs_until(&mut client, "t", "[hello there]").await;

    assert_eq!(
        client
            .show_environment(Some("MPX_TEST_GREETING"))
            .await
            .unwrap(),
        [EnvEntry {
            name: "MPX_TEST_GREETING".into(),
            value: Some("hello there".into()),
        }]
    );
    client
        .set_environment("MPX_TEST_GREETING", EnvChange::Unset)
        .await
        .unwrap();
    assert!(
        client
            .show_environment(Some("MPX_TEST_GREETING"))
            .await
            .is_err()
    );
    assert!(client.show_environment(None).await.unwrap().is_empty());
}

/// A variable the daemon inherits and the shell does not synthesize when it is absent. On Windows
/// `sh` (Git for Windows, MSYS2) derives `HOME` from the profile when it is unset, so `HOME` cannot
/// show a removal there; `USERPROFILE` is left alone.
#[cfg(windows)]
const INHERITED: &str = "USERPROFILE";
#[cfg(not(windows))]
const INHERITED: &str = "HOME";

#[tokio::test]
async fn a_removed_variable_is_kept_out_of_task_and_pty_panes_alike() {
    let (_daemon, mut client) = fresh().await;
    client
        .set_environment(INHERITED, EnvChange::Remove)
        .await
        .unwrap();
    let probe = format!("echo \"[${{{INHERITED}-unset}}]\"");
    client
        .spawn(task("t", "sh", &["-c", probe.as_str()]))
        .await
        .unwrap();
    logs_until(&mut client, "t", "[unset]").await;

    let script = format!("{probe}; sleep 5");
    client
        .spawn(pty("p", "sh", &["-c", script.as_str()]))
        .await
        .unwrap();
    logs_until(&mut client, "p", "[unset]").await;

    assert_eq!(
        client.show_environment(Some(INHERITED)).await.unwrap(),
        [EnvEntry {
            name: INHERITED.into(),
            value: None,
        }]
    );
}
