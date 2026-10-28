use mpx::layout::{Rect, WindowTab};
use mpx::tui::status::{StatusTarget, clock_12h, clock_hms, restored_notice, status_hit};

#[test]
fn clock_formats_hours_minutes_and_seconds() {
    assert_eq!(clock_hms(0, 0), "00:00:00");
    assert_eq!(clock_hms(3_723_999, 0), "01:02:03");
}

#[test]
fn clock_wraps_at_midnight() {
    assert_eq!(clock_hms((86_400 + 59) * 1000, 0), "00:00:59");
}

#[test]
fn clock_applies_the_local_offset_across_midnight() {
    let pdt = -7 * 3600;
    assert_eq!(
        clock_hms(3_723_000, pdt),
        "18:02:03",
        "01:02 UTC is the previous evening in PDT"
    );
    assert_eq!(
        clock_hms(82_800_000, 2 * 3600),
        "01:00:00",
        "23:00 UTC is past midnight at UTC+2"
    );
}

#[test]
fn twelve_hour_clock_maps_midnight_and_noon_to_twelve() {
    assert_eq!(clock_12h(0, 0), "12:00:00");
    assert_eq!(clock_12h(43_200_000, 0), "12:00:00");
    assert_eq!(clock_12h(46_800_000, 0), "01:00:00");
}

#[test]
fn restored_notice_names_the_snapshot_time() {
    assert_eq!(
        restored_notice(45_296_000, 0),
        "restored session from 12:34:56"
    );
}

#[test]
fn status_hit_detects_clicked_window_tab() {
    let tabs = vec![
        WindowTab {
            index: 0,
            name: "editor".into(),
            is_active: true,
        },
        WindowTab {
            index: 1,
            name: "server".into(),
            is_active: false,
        },
    ];
    // "[dev] 0:editor* 1:server- "
    // offset of [dev]  is 6 chars (0..6)
    // "0:editor* " is 10 chars (6..16)
    // "1:server- " is 10 chars (16..26)
    assert_eq!(status_hit("dev", &tabs, &[], false, 2), None);
    assert_eq!(status_hit("dev", &tabs, &[], false, 6), Some(StatusTarget::Window(0)));
    assert_eq!(status_hit("dev", &tabs, &[], false, 15), Some(StatusTarget::Window(0)));
    assert_eq!(status_hit("dev", &tabs, &[], false, 16), Some(StatusTarget::Window(1)));
    assert_eq!(status_hit("dev", &tabs, &[], false, 25), Some(StatusTarget::Window(1)));
    assert_eq!(status_hit("dev", &tabs, &[], false, 26), None);
}

#[test]
fn status_hit_detects_clicked_pane_when_no_tabs() {
    let panes = vec![
        ("main".to_string(), Rect::new(0, 0, 40, 23), true),
        ("worker".to_string(), Rect::new(40, 0, 40, 23), false),
    ];
    // "[sess] 0:main* 1:worker- "
    // offset of [sess]  is 7 chars (0..7)
    // "0:main* " is 8 chars (7..15)
    // "1:worker- " is 10 chars (15..25)
    assert_eq!(status_hit("sess", &[], &panes, false, 7), Some(StatusTarget::Pane("main")));
    assert_eq!(status_hit("sess", &[], &panes, false, 14), Some(StatusTarget::Pane("main")));
    assert_eq!(status_hit("sess", &[], &panes, false, 15), Some(StatusTarget::Pane("worker")));
    assert_eq!(status_hit("sess", &[], &panes, false, 24), Some(StatusTarget::Pane("worker")));
}
