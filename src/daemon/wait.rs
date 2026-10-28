use regex::Regex;
use std::time::Duration;

use super::DaemonState;
use super::state::Entry;
use crate::buffer::strip_ansi;
use crate::node::now_ms;
use crate::protocol::{AgentState, WaitCondition, WaitOutcome};

const POLL: Duration = Duration::from_millis(20);

enum Cond {
    Exit,
    Idle { quiet_ms: u64 },
    Match(Regex),
    State(AgentState),
}

/// A wait in progress: the condition plus what the pane showed when the request arrived.
struct Probe {
    cond: Cond,
    start: u64,
    screen: String,
}

fn satisfied(
    exit_code: Option<i32>,
    state: Option<AgentState>,
    matched: Option<String>,
) -> WaitOutcome {
    WaitOutcome {
        satisfied: true,
        exit_code,
        state,
        matched,
    }
}

impl Probe {
    fn new(state: &DaemonState, target: &str, condition: WaitCondition) -> Result<Self, String> {
        let cond = match condition {
            WaitCondition::Exit => Cond::Exit,
            WaitCondition::Idle { quiet_ms } => Cond::Idle { quiet_ms },
            WaitCondition::Match { pattern } => {
                Cond::Match(Regex::new(&pattern).map_err(|e| format!("Invalid pattern: {}", e))?)
            }
            WaitCondition::State { state: want } => Cond::State(want),
        };
        let (start, screen) =
            state.with_entry(target, |e| (e.node.total_written(), e.node.read_screen()))?;
        Ok(Self {
            cond,
            start,
            screen,
        })
    }

    fn evaluate(&self, daemon: &DaemonState, entry: &mut Entry) -> Option<WaitOutcome> {
        let now = now_ms();
        daemon.refresh(entry, now);
        match &self.cond {
            Cond::Exit => {
                (!entry.node.is_running()).then(|| satisfied(entry.node.exit_code(), None, None))
            }
            Cond::Idle { quiet_ms } => (now.saturating_sub(entry.node.last_output_ms())
                >= *quiet_ms)
                .then(|| satisfied(None, None, None)),
            Cond::Match(re) => {
                let fresh = strip_ansi(&entry.node.read_since(self.start).0);
                let text = format!("{}\n{}", self.screen, fresh);
                re.find(&text)
                    .map(|m| satisfied(None, None, Some(m.as_str().to_string())))
            }
            Cond::State(want) => {
                (entry.agent.state() == Some(*want)).then(|| satisfied(None, Some(*want), None))
            }
        }
    }

    fn check(&self, daemon: &DaemonState, name: &str) -> Option<WaitOutcome> {
        let outcome = daemon
            .with_entry(name, |entry| self.evaluate(daemon, entry))
            .ok()
            .flatten();
        if outcome.is_some() && matches!(self.cond, Cond::Exit) {
            daemon.settle(name);
        }
        outcome
    }
}

/// Blocks until the condition holds or `timeout_ms` elapses (`satisfied: false`).
pub(super) async fn wait_for(
    daemon: &DaemonState,
    target: &str,
    condition: WaitCondition,
    timeout_ms: Option<u64>,
) -> Result<WaitOutcome, String> {
    let probe = Probe::new(daemon, target, condition)?;
    let name = daemon.target_name(target)?;
    let run = async {
        loop {
            if let Some(outcome) = probe.check(daemon, &name) {
                return outcome;
            }
            tokio::time::sleep(POLL).await;
        }
    };
    match timeout_ms {
        None => Ok(run.await),
        Some(ms) => match tokio::time::timeout(Duration::from_millis(ms), run).await {
            Ok(outcome) => Ok(outcome),
            Err(_) => daemon.with_entry(&name, |entry| {
                let now = now_ms();
                daemon.refresh(entry, now);
                WaitOutcome {
                    satisfied: false,
                    exit_code: entry.node.exit_code(),
                    state: entry.agent.state(),
                    matched: None,
                }
            }),
        },
    }
}
