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

#[tokio::test]
async fn a_removed_variable_is_kept_out_of_task_and_pty_panes_alike() {
    let (_daemon, mut client) = fresh().await;
    client
        .set_environment("HOME", EnvChange::Remove)
        .await
        .unwrap();
    let probe = "echo \"[${HOME-unset}]\"";
    client.spawn(task("t", "sh", &["-c", probe])).await.unwrap();
    logs_until(&mut client, "t", "[unset]").await;

    let script = format!("{probe}; sleep 5");
    client
        .spawn(pty("p", "sh", &["-c", script.as_str()]))
        .await
        .unwrap();
    logs_until(&mut client, "p", "[unset]").await;

    assert_eq!(
        client.show_environment(Some("HOME")).await.unwrap(),
        [EnvEntry {
            name: "HOME".into(),
            value: None,
        }]
    );
}
