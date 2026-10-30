//! Session state behind the tmux surface: options, environment, paste buffers, `wait-for`
//! channels, client commands, window and pane moves, and `pipe-pane`.
use std::time::Duration;

use super::support::{connect, eventually, logs_until, pty, start, task};
use mpx::protocol::{ChannelAction, NewWindowOpts, StreamEvent, VarKind};

#[tokio::test]
async fn options_and_environment_are_separate_stores_and_can_be_unset() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.set_var(VarKind::Option, "@theme", Some("dark")).await.unwrap();
    client.set_var(VarKind::Option, "a", Some("1")).await.unwrap();
    client.set_var(VarKind::Environment, "FOO", Some("bar")).await.unwrap();

    let options = client.get_vars(VarKind::Option).await.unwrap();
    assert_eq!(
        options,
        [("@theme".into(), "dark".into()), ("a".into(), "1".into())]
    );
    assert_eq!(
        client.get_vars(VarKind::Environment).await.unwrap(),
        [("FOO".into(), "bar".into())]
    );

    client.set_var(VarKind::Option, "a", None).await.unwrap();
    assert_eq!(client.get_vars(VarKind::Option).await.unwrap().len(), 1);
}

#[tokio::test]
async fn session_environment_reaches_panes_created_later_but_not_earlier_ones() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client
        .spawn(task("before", "sh", &["-c", "sleep 0.3; echo val=${MPX_TEST_VAR:-unset}"]))
        .await
        .unwrap();
    client
        .set_var(VarKind::Environment, "MPX_TEST_VAR", Some("bar"))
        .await
        .unwrap();
    client
        .spawn(task("after", "sh", &["-c", "echo val=${MPX_TEST_VAR:-unset}"]))
        .await
        .unwrap();
    assert!(logs_until(&mut client, "after", "val=bar").await.contains("val=bar"));
    assert!(logs_until(&mut client, "before", "val=").await.contains("val=unset"));
}

#[tokio::test]
async fn buffers_stack_newest_first_and_a_named_buffer_is_replaced() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.set_buffer(None, "one").await.unwrap();
    client.set_buffer(None, "two").await.unwrap();
    client.set_buffer(Some("keep"), "old").await.unwrap();
    client.set_buffer(Some("keep"), "new").await.unwrap();

    let names: Vec<_> = client
        .list_buffers()
        .await
        .unwrap()
        .into_iter()
        .map(|b| (b.name, b.data))
        .collect();
    assert_eq!(
        names,
        [
            ("keep".to_string(), "new".to_string()),
            ("buffer1".to_string(), "two".to_string()),
            ("buffer0".to_string(), "one".to_string()),
        ]
    );
    assert_eq!(client.get_buffer(None).await.unwrap().data, "new");
    assert_eq!(client.get_buffer(Some("buffer0")).await.unwrap().data, "one");

    client.delete_buffer(None).await.unwrap();
    assert_eq!(client.get_buffer(None).await.unwrap().name, "buffer1");
    assert!(client.get_buffer(Some("missing")).await.is_err());
    assert!(client.delete_buffer(Some("missing")).await.is_err());
}

#[tokio::test]
async fn only_the_newest_fifty_buffers_are_kept() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for i in 0..55 {
        client.set_buffer(None, &i.to_string()).await.unwrap();
    }
    let buffers = client.list_buffers().await.unwrap();
    assert_eq!(buffers.len(), 50);
    assert_eq!(buffers[0].data, "54");
    assert_eq!(buffers[49].data, "5");
}

async fn finishes_within(
    task: tokio::task::JoinHandle<Result<(), String>>,
    wait: Duration,
) -> bool {
    matches!(tokio::time::timeout(wait, task).await, Ok(Ok(Ok(()))))
}

#[tokio::test]
async fn wait_for_blocks_until_signaled_and_remembers_an_early_signal() {
    let daemon = start().await;
    let mut signaler = connect(&daemon.endpoint).await;

    let waiter = {
        let mut client = connect(&daemon.endpoint).await;
        tokio::spawn(async move { client.wait_for("ready", ChannelAction::Wait).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!waiter.is_finished(), "returned before any signal");
    signaler.wait_for("ready", ChannelAction::Signal).await.unwrap();
    assert!(finishes_within(waiter, Duration::from_secs(2)).await);

    signaler.wait_for("early", ChannelAction::Signal).await.unwrap();
    let mut late = connect(&daemon.endpoint).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        late.wait_for("early", ChannelAction::Wait),
    )
    .await
    .expect("a signal sent first is not lost")
    .unwrap();
}

#[tokio::test]
async fn wait_for_locks_hand_over_in_order_and_unlocking_a_free_channel_fails() {
    let daemon = start().await;
    let mut first = connect(&daemon.endpoint).await;
    first.wait_for("lk", ChannelAction::Lock).await.unwrap();

    let second = {
        let mut client = connect(&daemon.endpoint).await;
        tokio::spawn(async move { client.wait_for("lk", ChannelAction::Lock).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!second.is_finished(), "the lock was already held");
    first.wait_for("lk", ChannelAction::Unlock).await.unwrap();
    assert!(finishes_within(second, Duration::from_secs(2)).await);

    let mut other = connect(&daemon.endpoint).await;
    other.wait_for("lk", ChannelAction::Unlock).await.unwrap();
    assert!(other.wait_for("lk", ChannelAction::Unlock).await.is_err());
}

#[tokio::test]
async fn client_commands_reach_terminals_and_no_other_subscriber() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("p", "sh", &["-c", "cat"])).await.unwrap();

    let (term_tx, mut term_rx) = tokio::sync::mpsc::channel(64);
    let terminal = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = terminal.stream_logs_into((None, None), true, term_tx, |e| e).await;
    });
    let (all_tx, mut all_rx) = tokio::sync::mpsc::channel(64);
    let everything = connect(&daemon.endpoint).await;
    tokio::spawn(async move {
        let _ = everything.stream_logs_into((None, None), false, all_tx, |e| e).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.client_command("detach-client").await.unwrap();
    let heard = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = term_rx.recv().await {
            if let StreamEvent::ClientCommand { line } = event {
                return Some(line);
            }
        }
        None
    })
    .await;
    assert_eq!(heard, Ok(Some("detach-client".to_string())));

    client.kill("p").await.unwrap();
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(500), all_rx.recv()).await
    {
        assert!(!matches!(event, StreamEvent::ClientCommand { .. }), "{event:?}");
    }
}

fn window(label: &str, pane: &str) -> NewWindowOpts {
    NewWindowOpts {
        name: Some(label.to_string()),
        spawn: task(pane, "cat", &[]),
        focus: true,
    }
}

async fn tabs(client: &mut mpx::client::Client) -> Vec<(String, bool)> {
    let (_, _, tabs) = client.layout(80, 24).await.unwrap();
    tabs.into_iter().map(|t| (t.name, t.is_active)).collect()
}

#[tokio::test]
async fn windows_swap_and_move_while_the_active_window_stays_the_same_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    for (label, pane) in [("a", "pa"), ("b", "pb"), ("c", "pc")] {
        client.new_window(window(label, pane)).await.unwrap();
    }
    client.select_window(0).await.unwrap();

    client.swap_window(0, 2).await.unwrap();
    assert_eq!(
        tabs(&mut client).await,
        [("c".into(), false), ("b".into(), false), ("a".into(), true)]
    );

    client.move_window(2, 0).await.unwrap();
    assert_eq!(
        tabs(&mut client).await,
        [("a".into(), true), ("c".into(), false), ("b".into(), false)]
    );
    assert!(client.swap_window(0, 9).await.is_err());
    assert!(client.move_window(9, 0).await.is_err());
}

#[tokio::test]
async fn join_pane_moves_a_pane_next_to_another_and_drops_the_emptied_window() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.new_window(window("a", "pa")).await.unwrap();
    client.new_window(window("b", "pb")).await.unwrap();

    client.join_pane("pb", "pa", true).await.unwrap();

    let (rects, _, tabs) = client.layout(80, 24).await.unwrap();
    assert_eq!(tabs.len(), 1, "window b had only that pane");
    let mut names: Vec<_> = rects.iter().map(|r| r.0.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["pa", "pb"]);
    let (left, right) = (&rects[0].1, &rects[1].1);
    assert_eq!(left.height, right.height, "side by side share the height");

    assert!(client.join_pane("pa", "pa", true).await.is_err());
    assert!(client.join_pane("nope", "pa", true).await.is_err());
}

#[tokio::test]
async fn pipe_pane_feeds_output_to_a_command_and_toggles_off() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    client.spawn(pty("piped", "cat", &[])).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("piped.txt");
    let command = format!("cat >> {}", out.display());

    client.pipe_pane("piped", Some(&command), false).await.unwrap();
    client.send_input("piped", "hello-pipe").await.unwrap();
    eventually(Duration::from_secs(5), || {
        std::fs::read_to_string(&out).ok().filter(|t| t.contains("hello-pipe"))
    })
    .await;

    // `-o` on a running pipe stops it instead of starting another.
    client.pipe_pane("piped", Some(&command), true).await.unwrap();
    let before = std::fs::read_to_string(&out).unwrap();
    client.send_input("piped", "after-stop").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(std::fs::read_to_string(&out).unwrap(), before);

    assert!(client.pipe_pane("missing", Some("cat"), false).await.is_err());
}

#[tokio::test]
async fn hooks_run_their_command_after_the_event_and_unknown_hooks_are_refused() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let mark = dir.path().join("hook.txt");
    let line = format!("run-shell \"echo fired >> {}\"", mark.display());

    client.set_var(VarKind::Hook, "after-new-window", Some(&line)).await.unwrap();
    assert!(client.set_var(VarKind::Hook, "after-nonsense", Some("x")).await.is_err());
    assert!(!mark.exists(), "setting a hook runs nothing");

    client.new_window(window("w", "hp")).await.unwrap();
    eventually(Duration::from_secs(5), || {
        std::fs::read_to_string(&mark).ok().filter(|t| t.contains("fired"))
    })
    .await;

    client.set_var(VarKind::Hook, "after-new-window", None).await.unwrap();
    let before = std::fs::read_to_string(&mark).unwrap();
    client.new_window(window("w2", "hp2")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(std::fs::read_to_string(&mark).unwrap(), before, "an unset hook stays quiet");
}

#[tokio::test]
async fn pane_exit_and_a_failed_request_fire_the_right_hooks() {
    let daemon = start().await;
    let mut client = connect(&daemon.endpoint).await;
    let dir = tempfile::tempdir().unwrap();
    let exited = dir.path().join("exited.txt");
    let split = dir.path().join("split.txt");
    for (hook, file) in [("pane-exited", &exited), ("after-split-window", &split)] {
        let line = format!("run-shell \"touch {}\"", file.display());
        client.set_var(VarKind::Hook, hook, Some(&line)).await.unwrap();
    }
    assert!(client
        .split(mpx::protocol::SplitOpts {
            target: Some("nope".into()),
            spawn: task("x", "cat", &[]),
            direction: mpx::layout::Direction::Horizontal,
            percent: None,
            focus: true,
        })
        .await
        .is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!split.exists(), "a failed split fires no after-split-window");

    client.spawn(task("short", "true", &[])).await.unwrap();
    eventually(Duration::from_secs(5), || exited.exists().then_some(())).await;
}
