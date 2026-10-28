use std::time::Duration;

use mpx::mcp::{PollStep, poll_step};
use mpx::protocol::{AgentInfo, AgentState};

const BASELINE: u64 = 1_000;
const PLENTY: Duration = Duration::from_secs(60);

fn agent(state: AgentState, since_ms: u64) -> AgentInfo {
    AgentInfo {
        harness: "omp".into(),
        state,
        session_id: None,
        detail: None,
        since_ms,
    }
}

#[test]
fn the_idle_state_from_before_the_prompt_does_not_finish_it() {
    let before = agent(AgentState::Idle, BASELINE);
    assert_eq!(
        poll_step(Some(&before), BASELINE, PLENTY),
        PollStep::Wait(Duration::from_millis(200)),
        "a slow-starting agent is still idle from before the prompt"
    );
}

#[test]
fn a_later_transition_into_a_settled_state_finishes_the_prompt() {
    for state in [AgentState::Idle, AgentState::NeedsInput, AgentState::Exited] {
        let after = agent(state, BASELINE + 1);
        assert_eq!(
            poll_step(Some(&after), BASELINE, PLENTY),
            PollStep::Finished(state)
        );
    }
}

#[test]
fn working_keeps_polling() {
    let working = agent(AgentState::Working, BASELINE + 5);
    assert_eq!(
        poll_step(Some(&working), BASELINE, Duration::from_millis(150)),
        PollStep::Wait(Duration::from_millis(150))
    );
}

#[test]
fn an_exhausted_deadline_times_out_with_the_current_state() {
    let working = agent(AgentState::Working, BASELINE + 5);
    assert_eq!(
        poll_step(Some(&working), BASELINE, Duration::ZERO),
        PollStep::TimedOut(Some(AgentState::Working))
    );
    assert_eq!(
        poll_step(None, BASELINE, Duration::ZERO),
        PollStep::TimedOut(None)
    );
}
