//! Wait conditions and their evaluation against what a pane has observed. Pure: the pane actor
//! supplies the observation and the clock.
use regex::Regex;

use crate::protocol::{AgentState, WaitCondition, WaitOutcome};

#[derive(Debug, Clone)]
pub enum Cond {
    Exit,
    Idle { quiet_ms: u64 },
    Match(Regex),
    State(AgentState),
}

impl Cond {
    pub fn from_condition(condition: WaitCondition) -> Result<Cond, String> {
        Ok(match condition {
            WaitCondition::Exit => Cond::Exit,
            WaitCondition::Idle { quiet_ms } => Cond::Idle { quiet_ms },
            WaitCondition::Match { pattern } => {
                Cond::Match(Regex::new(&pattern).map_err(|e| format!("Invalid pattern: {}", e))?)
            }
            WaitCondition::State { state } => Cond::State(state),
        })
    }

    /// Output time at which an idle wait can first be satisfied.
    pub fn idle_due(&self, last_output_ms: u64) -> Option<u64> {
        match self {
            Cond::Idle { quiet_ms } => Some(last_output_ms + quiet_ms),
            _ => None,
        }
    }

    pub fn watches_state(&self) -> bool {
        matches!(self, Cond::State(_))
    }
}

/// Pane facts a wait is checked against.
#[derive(Debug, Clone, Copy)]
pub struct Observed {
    pub now_ms: u64,
    pub last_output_ms: u64,
    /// `None` while running, `Some(code)` once exited.
    pub exit: Option<Option<i32>>,
    pub state: Option<AgentState>,
}

/// The outcome when the condition holds, or `None` while it does not. `text` yields the plain text
/// a `Match` searches (screen at the request plus output since).
pub fn satisfied(cond: &Cond, obs: &Observed, text: &dyn Fn() -> String) -> Option<WaitOutcome> {
    match cond {
        Cond::Exit => obs.exit.map(|code| done(code, None, None)),
        Cond::Idle { quiet_ms } => (obs.now_ms.saturating_sub(obs.last_output_ms) >= *quiet_ms)
            .then(|| done(None, None, None)),
        Cond::Match(re) => {
            let plain = text();
            re.find(&plain)
                .map(|m| done(None, None, Some(m.as_str().to_string())))
        }
        Cond::State(want) => (obs.state == Some(*want)).then(|| done(None, Some(*want), None)),
    }
}

/// Outcome reported when the timeout elapses first.
pub fn timed_out(obs: &Observed) -> WaitOutcome {
    WaitOutcome {
        satisfied: false,
        exit_code: obs.exit.flatten(),
        state: obs.state,
        matched: None,
    }
}

fn done(exit_code: Option<i32>, state: Option<AgentState>, matched: Option<String>) -> WaitOutcome {
    WaitOutcome {
        satisfied: true,
        exit_code,
        state,
        matched,
    }
}
