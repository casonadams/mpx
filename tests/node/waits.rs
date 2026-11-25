use mpx::node::waits::{Cond, MAX_IDLE_MS};
use mpx::protocol::WaitCondition;

#[test]
fn idle_periods_up_to_a_year_are_accepted_and_longer_ones_refused() {
    for quiet_ms in [0, 100, MAX_IDLE_MS] {
        assert!(Cond::from_condition(WaitCondition::Idle { quiet_ms }).is_ok());
    }
    for quiet_ms in [MAX_IDLE_MS + 1, u64::MAX] {
        assert!(Cond::from_condition(WaitCondition::Idle { quiet_ms }).is_err());
    }
}
