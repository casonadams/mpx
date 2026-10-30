//! Wait conditions and their evaluation against what a pane has observed. Pure: the pane actor
//! supplies the observation and the clock.
use regex::Regex;

use crate::protocol::{WaitCondition, WaitOutcome};

#[derive(Debug, Clone)]
pub enum Cond {
    Exit,
    Idle { quiet_ms: u64 },
    Match(Regex),
}

impl Cond {
    pub fn from_condition(condition: WaitCondition) -> Result<Cond, String> {
        Ok(match condition {
            WaitCondition::Exit => Cond::Exit,
            WaitCondition::Idle { quiet_ms } => Cond::Idle { quiet_ms },
            WaitCondition::Match { pattern } => {
                Cond::Match(Regex::new(&pattern).map_err(|e| format!("Invalid pattern: {}", e))?)
            }
        })
    }

    /// Output time at which an idle wait can first be satisfied.
    pub fn idle_due(&self, last_output_ms: u64) -> Option<u64> {
        match self {
            Cond::Idle { quiet_ms } => Some(last_output_ms + quiet_ms),
            _ => None,
        }
    }
}

/// Pane facts a wait is checked against.
#[derive(Debug, Clone, Copy)]
pub struct Observed {
    pub now_ms: u64,
    pub last_output_ms: u64,
    /// `None` while running, `Some(code)` once exited.
    pub exit: Option<Option<i32>>,
}

/// The outcome when the condition holds, or `None` while it does not. `text` yields the plain text
/// a `Match` searches (screen at the request plus output since).
pub fn satisfied(cond: &Cond, obs: &Observed, text: &dyn Fn() -> String) -> Option<WaitOutcome> {
    match cond {
        Cond::Exit => obs.exit.map(|code| done(code, None)),
        Cond::Idle { quiet_ms } => (obs.now_ms.saturating_sub(obs.last_output_ms) >= *quiet_ms)
            .then(|| done(None, None)),
        Cond::Match(re) => {
            let plain = text();
            re.find(&plain)
                .map(|m| done(None, Some(m.as_str().to_string())))
        }
    }
}

/// Outcome reported when the timeout elapses first.
pub fn timed_out(obs: &Observed) -> WaitOutcome {
    WaitOutcome {
        satisfied: false,
        exit_code: obs.exit.flatten(),
        matched: None,
    }
}

fn done(exit_code: Option<i32>, matched: Option<String>) -> WaitOutcome {
    WaitOutcome {
        satisfied: true,
        exit_code,
        matched,
    }
}
