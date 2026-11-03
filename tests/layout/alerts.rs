use std::time::{Duration, Instant};

use mpx::layout::tree::Direction;
use mpx::layout::Workspace;

fn two_windows() -> Workspace {
    let mut ws = Workspace::new("main".to_string());
    ws.add_or_split("pane1".to_string(), Direction::Horizontal);
    ws.new_window("second".to_string(), "pane2".to_string());
    ws.select_window(0);
    ws
}

#[test]
fn a_background_window_goes_silent_after_its_quiet_period() {
    let mut ws = two_windows();
    let t0 = Instant::now();
    ws.note_output("pane2", t0);
    assert_eq!(ws.next_silence(10), Some(t0 + Duration::from_secs(10)));
    assert!(!ws.expire_silence(10, t0 + Duration::from_secs(9)));
    assert!(ws.expire_silence(10, t0 + Duration::from_secs(11)));
    let tabs = ws.window_tabs();
    assert!(tabs[1].has_silence);
    assert!(!tabs[0].has_silence, "the active window never goes silent");
    assert_eq!(ws.next_silence(10), None, "a flagged window is not counting");
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
    assert_eq!(ws.synchronize(Some(7), Some(true)), Err("No window 7".to_string()));
}
