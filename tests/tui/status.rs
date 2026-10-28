use mpx::tui::status::{clock_12h, clock_hms, restored_notice};

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
