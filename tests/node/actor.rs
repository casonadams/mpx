use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mpx::node::actor::PaneActor;
use mpx::node::pane::{PaneCtx, PaneMsg, PaneNotice};
use mpx::node::sink::Sink;
use mpx::node::waits::Cond;
use mpx::node::{NodeError, Process, SpawnSpec};
use mpx::protocol::{StreamEvent, WaitCondition, WaitOutcome};
use tokio::sync::oneshot;

#[derive(Default)]
struct FakeProcess {
    pid: Option<u32>,
    input: Arc<Mutex<Vec<u8>>>,
    kills: Arc<AtomicUsize>,
}

impl Process for FakeProcess {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn send_input(&self, data: &[u8]) -> Result<(), NodeError> {
        self.input.lock().unwrap().extend_from_slice(data);
        Ok(())
    }

    fn kill(&self) -> Result<(), NodeError> {
        self.kills.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Pane {
    actor: PaneActor,
    input: Arc<Mutex<Vec<u8>>>,
    kills: Arc<AtomicUsize>,
    notices: Arc<Mutex<Vec<PaneNotice>>>,
}

fn pane_with(pid: Option<u32>) -> Pane {
    let process = FakeProcess {
        pid,
        ..FakeProcess::default()
    };
    let (input, kills) = (Arc::clone(&process.input), Arc::clone(&process.kills));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notices);
    let ctx = PaneCtx {
        notify: Arc::new(move |notice| sink.lock().unwrap().push(notice)),
        relay: Arc::new(AtomicBool::new(false)),
        pacing: Arc::default(),
    };
    let spec = SpawnSpec {
        name: "pane".to_string(),
        id: 1,
        command: "sh".to_string(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        term: "xterm-256color".to_string(),
        scrollback: 100_000,
    };
    Pane {
        actor: PaneActor::new(&spec, Box::new(process), None, ctx),
        input,
        kills,
        notices,
    }
}

fn plain() -> Pane {
    pane_with(None)
}

fn output(bytes: &[u8]) -> PaneMsg {
    PaneMsg::Output(bytes.to_vec())
}

fn subscribe(
    p: &mut Pane,
    since: Option<u64>,
) -> tokio::sync::mpsc::Receiver<StreamEvent> {
    let (sink, rx) = Sink::channel(None);
    let (reply, ack) = oneshot::channel();
    p.actor.handle(PaneMsg::Subscribe { since, sink, reply });
    assert!(ack.blocking_recv().is_ok());
    rx
}

fn wait_msg(
    cond: WaitCondition,
    timeout: Option<Duration>,
) -> (PaneMsg, oneshot::Receiver<WaitOutcome>) {
    let (reply, rx) = oneshot::channel();
    let cond = Cond::from_condition(cond).expect("valid condition");
    (PaneMsg::Wait { cond, timeout, reply }, rx)
}

fn outputs(rx: &mut tokio::sync::mpsc::Receiver<StreamEvent>) -> Vec<(String, u64, u64)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            StreamEvent::Output {
                data, offset, next, ..
            } => Some((data, offset, next)),
            _ => None,
        })
        .collect()
}

#[test]
fn subscriber_receives_contiguous_offsets_across_split_utf8() {
    let mut p = plain();
    let mut rx = subscribe(&mut p, None);
    p.actor.handle(output(b"hello "));
    p.actor.handle(output(b"world"));
    p.actor.handle(output(&[0xC3]));
    p.actor.handle(output(&[0xA9]));
    p.actor.handle(output(b"!"));
    assert_eq!(
        outputs(&mut rx),
        vec![
            ("hello ".to_string(), 0, 6),
            ("world".to_string(), 6, 11),
            ("é".to_string(), 11, 13),
            ("!".to_string(), 13, 14),
        ]
    );
}

#[test]
fn subscriber_since_replays_from_offset_then_continues_live() {
    let mut p = plain();
    p.actor.handle(output(b"abcdef"));
    let mut rx = subscribe(&mut p, Some(2));
    p.actor.handle(output(b"gh"));
    assert_eq!(
        outputs(&mut rx),
        vec![("cdef".to_string(), 2, 6), ("gh".to_string(), 6, 8)]
    );
}

#[test]
fn exit_reaches_subscribers() {
    let mut p = plain();
    let mut rx = subscribe(&mut p, None);
    p.actor.handle(PaneMsg::Exited(Some(0)));

    let events: Vec<StreamEvent> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(
        events.as_slice(),
        [StreamEvent::Exit { code: Some(0), .. }]
    ));
    assert!(matches!(
        p.notices.lock().unwrap().as_slice(),
        [PaneNotice::Exited { code: Some(0), .. }]
    ));
}

#[test]
fn match_wait_resolves_on_the_output_that_matches() {
    let mut p = plain();
    let (msg, mut rx) = wait_msg(
        WaitCondition::Match {
            pattern: "ready".to_string(),
        },
        None,
    );
    p.actor.handle(msg);
    p.actor.handle(output(b"not yet"));
    assert!(rx.try_recv().is_err(), "no match yet");
    p.actor.handle(output(b"ready now"));
    let outcome = rx.try_recv().expect("resolved by output");
    assert!(outcome.satisfied);
    assert_eq!(outcome.matched.as_deref(), Some("ready"));
}

#[test]
fn terminal_query_is_answered_on_the_process_input() {
    let mut p = plain();
    p.actor.handle(output(b"\x1b[c"));
    assert!(
        p.input.lock().unwrap().starts_with(b"\x1b[?"),
        "device attributes answered"
    );
}

#[test]
fn kill_is_answered_when_the_process_exits() {
    let mut p = plain();
    let (reply, mut done) = oneshot::channel();
    p.actor.handle(PaneMsg::Kill { reply });
    assert_eq!(p.kills.load(Ordering::SeqCst), 1);
    assert!(done.try_recv().is_err(), "still running");
    p.actor.handle(PaneMsg::Exited(None));
    assert!(done.try_recv().is_ok());

    let (reply, mut again) = oneshot::channel();
    p.actor.handle(PaneMsg::Kill { reply });
    assert!(again.try_recv().is_ok(), "exited pane answers at once");
    assert_eq!(p.kills.load(Ordering::SeqCst), 1, "no second kill");
}

#[test]
fn input_to_an_exited_pane_is_refused() {
    let mut p = plain();
    p.actor.handle(PaneMsg::Exited(Some(0)));
    let (reply, mut sent) = oneshot::channel();
    p.actor.handle(PaneMsg::Input {
        build: Box::new(|_| b"late".to_vec()),
        reply,
    });
    assert!(sent.try_recv().unwrap().is_err());
    assert!(p.input.lock().unwrap().is_empty());
}
