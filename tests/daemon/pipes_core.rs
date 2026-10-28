use std::time::Duration;

use mpx::daemon::pipes::{PipeMsg, PipesCore, ReadArgs, spawn};
use mpx::protocol::{PipeInfo, PipeMessage};
use tokio::sync::oneshot;
use tokio::time::Instant;

type Answer = Result<(Vec<PipeMessage>, u64), String>;

fn write(core: &mut PipesCore, name: &str, data: &str) -> PipeMessage {
    let (reply, mut rx) = oneshot::channel();
    core.handle(PipeMsg::Write {
        name: name.to_string(),
        data: data.to_string(),
        reply,
    });
    rx.try_recv().expect("write answered")
}

fn read(core: &mut PipesCore, name: &str, args: ReadArgs) -> oneshot::Receiver<Answer> {
    let (reply, rx) = oneshot::channel();
    core.handle(PipeMsg::Read {
        name: name.to_string(),
        args,
        reply,
    });
    rx
}

fn timed(wait_ms: u64) -> ReadArgs {
    ReadArgs {
        wait_ms: Some(wait_ms),
        ..ReadArgs::default()
    }
}

fn list(core: &mut PipesCore) -> Vec<PipeInfo> {
    let (reply, mut rx) = oneshot::channel();
    core.handle(PipeMsg::List { reply });
    rx.try_recv().expect("list answered")
}

fn data(messages: &[PipeMessage]) -> Vec<&str> {
    messages.iter().map(|m| m.data.as_str()).collect()
}

#[test]
fn write_creates_the_pipe_and_numbers_its_messages() {
    let mut core = PipesCore::new(None);
    assert_eq!(write(&mut core, "bus", "a").seq, 1);
    assert_eq!(write(&mut core, "bus", "b").seq, 2);
    let info = list(&mut core);
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].name, "bus");
    assert_eq!(info[0].messages, 2);
    assert_eq!(info[0].next_seq, 3);
}

#[test]
fn untimed_read_answers_at_once_with_the_messages_after_its_cursor() {
    let mut core = PipesCore::new(None);
    write(&mut core, "bus", "a");
    write(&mut core, "bus", "b");
    let mut rx = read(
        &mut core,
        "bus",
        ReadArgs {
            since: Some(1),
            ..ReadArgs::default()
        },
    );
    let (messages, next) = rx.try_recv().unwrap().unwrap();
    assert_eq!(data(&messages), vec!["b"]);
    assert_eq!(next, 2);
}

#[test]
fn read_of_an_unknown_pipe_creates_it_empty() {
    let mut core = PipesCore::new(None);
    let mut rx = read(&mut core, "fresh", ReadArgs::default());
    assert_eq!(rx.try_recv().unwrap().unwrap(), (Vec::new(), 0));
    assert_eq!(list(&mut core)[0].name, "fresh");
}

#[test]
fn timed_read_is_woken_by_a_write_beyond_its_cursor() {
    let mut core = PipesCore::new(None);
    write(&mut core, "bus", "a");
    let mut rx = read(
        &mut core,
        "bus",
        ReadArgs {
            since: Some(1),
            wait_ms: Some(1_000),
            ..ReadArgs::default()
        },
    );
    assert!(rx.try_recv().is_err(), "nothing beyond the cursor yet");
    write(&mut core, "bus", "b");
    let (messages, next) = rx.try_recv().unwrap().unwrap();
    assert_eq!(data(&messages), vec!["b"]);
    assert_eq!(next, 2);
}

#[test]
fn write_to_another_pipe_does_not_wake_a_timed_read() {
    let mut core = PipesCore::new(None);
    let mut rx = read(&mut core, "bus", timed(1_000));
    write(&mut core, "other", "x");
    assert!(rx.try_recv().is_err());
    write(&mut core, "bus", "y");
    assert_eq!(data(&rx.try_recv().unwrap().unwrap().0), vec!["y"]);
}

#[test]
fn timed_read_expires_at_its_deadline_with_nothing_new() {
    let mut core = PipesCore::new(None);
    let mut rx = read(&mut core, "bus", timed(1_000));
    let now = Instant::now();
    assert!(core.next_deadline().is_some());

    core.expire(now);
    assert!(rx.try_recv().is_err(), "deadline not reached");

    core.expire(now + Duration::from_secs(2));
    assert_eq!(rx.try_recv().unwrap().unwrap(), (Vec::new(), 0));
    assert_eq!(core.next_deadline(), None);
}

#[test]
fn expiry_answers_only_the_reads_that_are_due() {
    let mut core = PipesCore::new(None);
    let mut short = read(&mut core, "bus", timed(500));
    let mut long = read(&mut core, "bus", timed(5_000));
    core.expire(Instant::now() + Duration::from_secs(1));
    assert!(short.try_recv().is_ok());
    assert!(long.try_recv().is_err());
    assert!(core.next_deadline().is_some(), "long read still pending");
}

#[test]
fn list_reports_every_pipe_with_its_retained_messages() {
    let mut core = PipesCore::new(None);
    let (reply, _ack) = oneshot::channel();
    core.handle(PipeMsg::Create {
        name: "small".to_string(),
        capacity: Some(10),
        reply,
    });
    for text in ["aaaa", "bbbb", "cccc"] {
        write(&mut core, "small", text);
    }
    write(&mut core, "wide", "z");

    let mut info = list(&mut core);
    info.sort_by(|a, b| a.name.cmp(&b.name));
    let summary: Vec<(String, usize, u64)> = info
        .into_iter()
        .map(|p| (p.name, p.messages, p.next_seq))
        .collect();
    assert_eq!(
        summary,
        vec![("small".to_string(), 2, 4), ("wide".to_string(), 1, 2)]
    );
}

#[tokio::test]
async fn spawned_actor_wakes_a_timed_read_on_write() {
    let pipes = spawn(None);
    let reader = pipes.clone();
    let read = tokio::spawn(async move {
        reader
            .read(
                "bus",
                ReadArgs {
                    wait_ms: Some(5_000),
                    ..ReadArgs::default()
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    pipes.write("bus", "ping").await.unwrap();
    let (messages, _) = tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .expect("woken in time")
        .unwrap()
        .unwrap();
    assert_eq!(data(&messages), vec!["ping"]);
}

#[tokio::test]
async fn spawned_actor_expires_a_timed_read_and_lists_pipes() {
    let pipes = spawn(None);
    pipes.write("bus", "a").await.unwrap();
    let started = Instant::now();
    let (messages, next) = pipes.read("quiet", timed(50)).await.expect("read answered");
    assert!(messages.is_empty());
    assert_eq!(next, 0);
    assert!(started.elapsed() >= Duration::from_millis(40));

    let mut names: Vec<String> = pipes.list().await.into_iter().map(|p| p.name).collect();
    names.sort();
    assert_eq!(names, vec!["bus".to_string(), "quiet".to_string()]);
}
