use std::time::{Duration, Instant};

use super::two_windows;

#[test]
fn a_background_window_goes_silent_after_its_quiet_period() {
    let mut ws = two_windows();
    let t0 = Instant::now();
    ws.note_output("pane2", t0);
    assert_eq!(ws.next_silence(10), Some(t0 + Duration::from_secs(10)));
    assert!(
        ws.expire_silence(10, t0 + Duration::from_secs(9))
            .is_empty()
    );
    assert_eq!(ws.expire_silence(10, t0 + Duration::from_secs(11)), vec![1]);
    let tabs = ws.window_tabs();
    assert!(tabs[1].has_silence);
    assert!(!tabs[0].has_silence, "the active window never goes silent");
    assert_eq!(
        ws.next_silence(10),
        None,
        "a flagged window is not counting"
    );
}

#[test]
fn output_clears_the_silence_flag_and_selecting_the_window_does_too() {
    let mut ws = two_windows();
    let t0 = Instant::now();
    ws.note_output("pane2", t0);
    ws.expire_silence(10, t0 + Duration::from_secs(11));
    ws.note_output("pane2", t0 + Duration::from_secs(12));
    assert!(!ws.window_tabs()[1].has_silence);
    ws.expire_silence(10, t0 + Duration::from_secs(30));
    assert!(ws.window_tabs()[1].has_silence);
    ws.select_window(1);
    assert!(!ws.window_tabs()[1].has_silence);
}

#[test]
fn synchronize_toggles_the_active_window_or_a_named_one() {
    let mut ws = two_windows();
    assert!(ws.synchronize(None, None).is_ok());
    assert!(ws.window_tabs()[0].synchronized);
    assert!(ws.synchronize(Some(1), Some(true)).is_ok());
    assert!(ws.window_tabs()[1].synchronized);
    assert!(ws.synchronize(None, Some(false)).is_ok());
    assert!(!ws.window_tabs()[0].synchronized);
    assert_eq!(
        ws.synchronize(Some(7), Some(true)),
        Err("No window 7".to_string())
    );
}

#[test]
fn output_from_a_pane_in_no_window_starts_no_silence_timer() {
    let mut ws = two_windows();
    ws.note_output("ghost", Instant::now());
    assert_eq!(ws.next_silence(10), None);
}
