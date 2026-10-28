use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mpx::agents::Harness;
use mpx::node::actor::PaneActor;
use mpx::node::agent::Report;
use mpx::node::pane::{PaneCtx, PaneMsg, PaneNotice};
use mpx::node::sink::Sink;
use mpx::node::waits::Cond;
use mpx::node::{NodeError, Process, SpawnSpec};
use mpx::protocol::{AgentState, StreamEvent, WaitCondition, WaitOutcome};
use tokio::sync::oneshot;
use tokio::time::Instant;

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

fn pane_with(pid: Option<u32>, harnesses: Vec<Harness>, idle_ms: u64) -> Pane {
    let process = FakeProcess {
        pid,
        ..FakeProcess::default()
    };
    let (input, kills) = (Arc::clone(&process.input), Arc::clone(&process.kills));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notices);
    let ctx = PaneCtx {
        notify: Arc::new(move |notice| sink.lock().unwrap().push(notice)),
        harnesses: Arc::new(harnesses),
        idle_ms,
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
    };
    Pane {
        actor: PaneActor::new(&spec, Box::new(process), None, ctx),
        input,
        kills,
        notices,
    }
}

fn plain() -> Pane {
    pane_with(None, Vec::new(), 60_000)
}

/// Harness that detects this test binary, so the pane's own process reads as that agent.
fn self_harness(name: &str) -> Vec<Harness> {
    let exe = std::env::current_exe().expect("current exe");
    let detect = exe
        .file_name()
        .expect("exe name")
        .to_string_lossy()
        .into_owned();
    vec![Harness {
        name: name.to_string(),
        detect: vec![detect],
        command: vec![name.to_string()],
        resume: Vec::new(),
        session_id_args: Vec::new(),
        store: None,
    }]
}

fn output(bytes: &[u8]) -> PaneMsg {
    PaneMsg::Output(bytes.to_vec())
}

fn report(harness: &str, state: Option<AgentState>) -> Report {
    Report {
        harness: Some(harness.to_string()),
        state,
        ..Report::default()
    }
}

fn wait_msg(
    condition: WaitCondition,
    timeout: Option<Duration>,
) -> (PaneMsg, oneshot::Receiver<WaitOutcome>) {
    let (reply, rx) = oneshot::channel();
    let cond = Cond::from_condition(condition).expect("valid condition");
    (
        PaneMsg::Wait {
            cond,
            timeout,
            reply,
        },
        rx,
    )
}

fn subscribe(p: &mut Pane, since: Option<u64>) -> tokio::sync::mpsc::Receiver<StreamEvent> {
    let (sink, rx) = Sink::channel(None);
    let (reply, _ack) = oneshot::channel();
    p.actor.handle(PaneMsg::Subscribe { since, sink, reply });
    rx
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

fn agent_state(p: &mut Pane) -> Option<AgentState> {
    let (reply, mut rx) = oneshot::channel();
    p.actor.handle(PaneMsg::Info { full: false, reply });
    rx.try_recv().expect("info answered").agent.map(|a| a.state)
}

fn agent_harness(p: &mut Pane) -> Option<String> {
    let (reply, mut rx) = oneshot::channel();
    p.actor.handle(PaneMsg::Info { full: false, reply });
    rx.try_recv()
        .expect("info answered")
        .agent
        .map(|a| a.harness)
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
fn exit_reaches_subscribers_after_the_agent_transition() {
    let mut p = pane_with(None, self_harness("fake"), 60_000);
    let mut rx = subscribe(&mut p, None);
    p.actor.handle(PaneMsg::Report {
        report: report("fake", Some(AgentState::Working)),
        reply: oneshot::channel().0,
    });
    p.actor.handle(PaneMsg::Exited(Some(0)));

    let events: Vec<StreamEvent> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(
        events.as_slice(),
        [
            StreamEvent::Agent { agent: Some(working), .. },
            StreamEvent::Agent { agent: Some(exited), .. },
            StreamEvent::Exit { code: Some(0), .. },
        ] if working.state == AgentState::Working && exited.state == AgentState::Exited
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
fn exit_wait_resolves_with_the_code_and_later_waits_resolve_at_once() {
    let mut p = plain();
    let (msg, mut rx) = wait_msg(WaitCondition::Exit, None);
    p.actor.handle(msg);
    assert!(rx.try_recv().is_err());
    p.actor.handle(PaneMsg::Exited(Some(7)));
    let outcome = rx.try_recv().expect("resolved by exit");
    assert!(outcome.satisfied);
    assert_eq!(outcome.exit_code, Some(7));

    let (msg, mut late) = wait_msg(WaitCondition::Exit, None);
    p.actor.handle(msg);
    assert_eq!(late.try_recv().unwrap().exit_code, Some(7));
}

#[test]
fn timed_wait_resolves_unsatisfied_at_its_deadline() {
    let mut p = plain();
    let (msg, mut rx) = wait_msg(
        WaitCondition::Match {
            pattern: "never".to_string(),
        },
        Some(Duration::from_millis(100)),
    );
    p.actor.handle(msg);
    p.actor.on_timer(Instant::now());
    assert!(rx.try_recv().is_err(), "deadline not reached");
    p.actor
        .on_timer(Instant::now() + Duration::from_millis(200));
    let outcome = rx.try_recv().expect("timed out");
    assert!(!outcome.satisfied);
    assert_eq!(outcome.matched, None);
}

#[test]
fn idle_wait_resolves_once_output_has_been_quiet_long_enough() {
    let mut p = plain();
    p.actor.handle(output(b"x"));
    let (msg, mut rx) = wait_msg(WaitCondition::Idle { quiet_ms: 40 }, None);
    p.actor.handle(msg);
    p.actor.on_timer(Instant::now());
    assert!(rx.try_recv().is_err(), "output is still fresh");
    std::thread::sleep(Duration::from_millis(80));
    p.actor.on_timer(Instant::now());
    assert!(rx.try_recv().expect("quiet").satisfied);
}

#[test]
fn state_wait_is_resolved_by_a_report() {
    let mut p = plain();
    let (msg, mut rx) = wait_msg(
        WaitCondition::State {
            state: AgentState::NeedsInput,
        },
        None,
    );
    p.actor.handle(msg);
    assert!(rx.try_recv().is_err());
    let (reply, mut ack) = oneshot::channel();
    p.actor.handle(PaneMsg::Report {
        report: report("fake", Some(AgentState::NeedsInput)),
        reply,
    });
    assert!(ack.try_recv().is_ok(), "report acknowledged");
    let outcome = rx.try_recv().expect("state reached");
    assert_eq!(outcome.state, Some(AgentState::NeedsInput));
}

#[test]
fn bell_moves_an_agent_to_needs_input_and_input_resumes_it() {
    let mut p = pane_with(None, Vec::new(), 60_000);
    p.actor.handle(PaneMsg::Report {
        report: report("claude", Some(AgentState::Working)),
        reply: oneshot::channel().0,
    });
    p.actor.handle(output(b"\x07"));
    assert_eq!(agent_state(&mut p), Some(AgentState::NeedsInput));

    let (reply, mut sent) = oneshot::channel();
    p.actor.handle(PaneMsg::Input {
        build: Box::new(|_| b"y\r".to_vec()),
        reply,
    });
    assert!(sent.try_recv().unwrap().is_ok());
    assert_eq!(p.input.lock().unwrap().as_slice(), b"y\r");
    assert_eq!(agent_state(&mut p), Some(AgentState::Working));
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
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn probe_detects_the_foreground_agent_on_refresh() {
    let mut p = pane_with(Some(std::process::id()), self_harness("fake"), 60_000);
    assert_eq!(agent_harness(&mut p), Some("fake".to_string()));
    assert_eq!(agent_state(&mut p), Some(AgentState::Working));
}

#[test]
fn reported_state_is_authoritative_over_probe() {
    let mut p = pane_with(Some(std::process::id()), self_harness("fake"), 60_000);
    p.actor.handle(PaneMsg::Report {
        report: report("claude", Some(AgentState::NeedsInput)),
        reply: oneshot::channel().0,
    });
    assert_eq!(agent_harness(&mut p), Some("claude".to_string()));
    assert_eq!(agent_state(&mut p), Some(AgentState::NeedsInput));
}

#[test]
fn refresh_clears_an_unhooked_agent_that_is_no_longer_detected() {
    let mut p = plain();
    let mut rx = subscribe(&mut p, None);
    p.actor.handle(PaneMsg::Report {
        report: report("claude", None),
        reply: oneshot::channel().0,
    });
    p.actor.handle(PaneMsg::Info {
        full: false,
        reply: oneshot::channel().0,
    });
    let harnesses: Vec<Option<String>> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            StreamEvent::Agent { agent, .. } => Some(agent.map(|a| a.harness)),
            _ => None,
        })
        .collect();
    assert_eq!(harnesses, vec![Some("claude".to_string()), None]);
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unhooked_agent_goes_idle_after_its_quiet_period() {
    let mut p = pane_with(Some(std::process::id()), self_harness("claude"), 150);
    p.actor.handle(PaneMsg::Report {
        report: report("claude", None),
        reply: oneshot::channel().0,
    });
    p.actor.handle(output(b"busy"));
    p.actor.on_timer(Instant::now());
    assert_eq!(agent_state(&mut p), Some(AgentState::Working));
    std::thread::sleep(Duration::from_millis(200));
    p.actor.on_timer(Instant::now());
    assert_eq!(agent_state(&mut p), Some(AgentState::Idle));
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
