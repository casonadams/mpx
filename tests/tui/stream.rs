use mpx::tui::stream::{OutputPlan, plan_output};

#[test]
fn a_chunk_starting_at_the_expected_offset_is_fed() {
    assert_eq!(plan_output(Some(100), 100, 110), OutputPlan::Feed);
}

#[test]
fn a_chunk_past_the_expected_offset_is_a_gap_to_fetch() {
    assert_eq!(plan_output(Some(100), 105, 115), OutputPlan::Fetch);
}

#[test]
fn a_chunk_straddling_the_expected_offset_is_fetched_not_sliced() {
    assert_eq!(plan_output(Some(100), 95, 105), OutputPlan::Fetch);
}

#[test]
fn a_chunk_already_covered_is_skipped() {
    assert_eq!(plan_output(Some(100), 80, 100), OutputPlan::Skip);
    assert_eq!(plan_output(Some(100), 90, 95), OutputPlan::Skip);
}

#[test]
fn a_pane_without_a_baseline_skips_output_its_load_will_cover() {
    assert_eq!(plan_output(None, 0, 10), OutputPlan::Skip);
}

#[test]
fn an_in_order_burst_is_fed_chunk_by_chunk() {
    let chunks = [(0, 4), (4, 9), (9, 12)];
    let mut expected = Some(0);
    for (offset, next) in chunks {
        assert_eq!(plan_output(expected, offset, next), OutputPlan::Feed);
        expected = Some(next);
    }
    assert_eq!(expected, Some(12));
}
