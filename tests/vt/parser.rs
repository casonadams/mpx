use mpx::keys::TermModes;
use mpx::vt::grid::{CellStyle, Color};
use mpx::vt::{Alert, VtScreen};

fn feed(vt: &mut VtScreen, bytes: &str) {
    vt.feed(bytes.as_bytes());
}

fn row(vt: &VtScreen, y: usize) -> String {
    let grid = vt.grid();
    (0..grid.cols)
        .map(|x| grid.get_cell(x, y).unwrap().c)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn cursor(vt: &VtScreen) -> (usize, usize) {
    (vt.grid().cursor.x, vt.grid().cursor.y)
}

#[test]
fn plain_text_appears_in_snapshot() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "Hello from MPX\r\nSecond line");
    let snap = vt.snapshot();
    assert!(snap.contains("Hello from MPX"));
    assert!(snap.contains("Second line"));
}

#[test]
fn cup_moves_cursor_and_writes_at_position() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;5HX");
    assert_eq!(vt.grid().cursor.x, 5);
    assert_eq!(vt.grid().cursor.y, 1);
    assert_eq!(vt.grid().get_cell(4, 1).unwrap().c, 'X');
}

#[test]
fn sgr_sets_style_and_reset_clears_it() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[1;31mRED\x1b[0m");
    let cell = vt.grid().get_cell(0, 0).unwrap();
    assert_eq!(cell.c, 'R');
    assert_eq!(cell.style.fg, Some(Color::Red));
    assert!(cell.style.bold);
    assert_eq!(vt.grid().current_style, CellStyle::default());
}

#[test]
fn dsr_and_device_attributes_produce_responses() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[3;7H\x1b[6n");
    assert_eq!(vt.take_responses(), vec![b"\x1b[3;7R".to_vec()]);

    feed(&mut vt, "\x1b[c");
    assert_eq!(vt.take_responses(), vec![b"\x1b[?62;c".to_vec()]);
}

#[test]
fn scrolling_region_and_scroll_down_move_lines() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r");
    assert_eq!(vt.grid().scroll_top, 1);
    assert_eq!(vt.grid().scroll_bottom, 3);

    feed(&mut vt, "\x1b[2;1Hline2");
    assert_eq!(vt.grid().get_cell(0, 1).unwrap().c, 'l');

    feed(&mut vt, "\x1b[1T");
    assert_eq!(vt.grid().get_cell(0, 2).unwrap().c, 'l');
}

#[test]
fn modes_start_unset() {
    let vt = VtScreen::new(20, 5);
    assert_eq!(vt.modes(), TermModes::default());
}

#[test]
fn private_modes_set_and_reset_input_flags() {
    let mut vt = VtScreen::new(20, 5);

    feed(&mut vt, "\x1b[?1h");
    assert!(vt.modes().app_cursor);
    feed(&mut vt, "\x1b[?1l");
    assert!(!vt.modes().app_cursor);

    feed(&mut vt, "\x1b[?2004h");
    assert!(vt.modes().bracketed_paste);
    feed(&mut vt, "\x1b[?2004l");
    assert!(!vt.modes().bracketed_paste);

    feed(&mut vt, "\x1b[?1004h");
    assert!(vt.modes().focus_events);
    feed(&mut vt, "\x1b[?1004l");
    assert!(!vt.modes().focus_events);
}

#[test]
fn combined_private_mode_sets_every_listed_flag() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[?1;2004;1004h");
    let modes = vt.modes();
    assert!(modes.app_cursor && modes.bracketed_paste && modes.focus_events);
}

#[test]
fn osc_zero_and_two_set_title() {
    let mut vt = VtScreen::new(20, 5);
    assert_eq!(vt.title(), None);

    feed(&mut vt, "\x1b]0;hello\x07");
    assert_eq!(vt.title(), Some("hello"));

    feed(&mut vt, "\x1b]2;a;b\x1b\\");
    assert_eq!(vt.title(), Some("a;b"));
}

#[test]
fn osc_one_does_not_set_title() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]1;icon\x07");
    assert_eq!(vt.title(), None);
}

#[test]
fn bell_is_reported_once_and_drained() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x07");
    assert_eq!(vt.take_alerts(), vec![Alert::Bell]);
    assert!(vt.take_alerts().is_empty());
}

#[test]
fn osc_nine_is_a_notification() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]9;build done\x07");
    assert_eq!(vt.take_alerts(), vec![Alert::Notify("build done".into())]);
}

#[test]
fn osc_nine_rejoins_semicolons_in_message() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]9;a;b\x07");
    assert_eq!(vt.take_alerts(), vec![Alert::Notify("a;b".into())]);
}

#[test]
fn osc_nine_four_progress_is_ignored() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]9;4;1;50\x07");
    assert!(vt.take_alerts().is_empty());
}

#[test]
fn osc_777_notify_combines_title_and_body() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]777;notify;Title;Body text\x07");
    assert_eq!(
        vt.take_alerts(),
        vec![Alert::Notify("Title: Body text".into())]
    );
}

#[test]
fn osc_777_notify_without_body_uses_title_only() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b]777;notify;Only\x07");
    assert_eq!(vt.take_alerts(), vec![Alert::Notify("Only".into())]);
}

#[test]
fn alerts_drain_in_arrival_order() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x07\x1b]9;done\x07\x07");
    assert_eq!(
        vt.take_alerts(),
        vec![Alert::Bell, Alert::Notify("done".into()), Alert::Bell]
    );
}

#[test]
fn scrollback_text_includes_history_and_visible_rows() {
    let mut vt = VtScreen::new(10, 3);
    feed(&mut vt, "one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(vt.scrollback_text(None), "one\ntwo\nthree\nfour");
}

#[test]
fn scrollback_tail_keeps_last_lines() {
    let mut vt = VtScreen::new(10, 3);
    feed(&mut vt, "one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(vt.scrollback_text(Some(2)), "three\nfour");
    assert_eq!(vt.scrollback_text(Some(0)), "");
    assert_eq!(vt.scrollback_text(Some(100)), "one\ntwo\nthree\nfour");
}

#[test]
fn scrollback_trims_trailing_spaces_and_blank_lines() {
    let mut vt = VtScreen::new(10, 3);
    feed(&mut vt, "ab   \r\n\r\n\r\n");
    assert_eq!(vt.scrollback_text(None), "ab");
}

#[test]
fn scrollback_tail_preserves_internal_blank_lines() {
    let mut vt = VtScreen::new(10, 4);
    feed(&mut vt, "line1\r\n\r\nline2\r\nline3\r\n\r\n");
    assert_eq!(vt.scrollback_text(Some(3)), "\nline2\nline3");
}

#[test]
fn cha_hpa_vpa_set_absolute_cursor_position() {
    let cases: &[(&str, (usize, usize))] = &[
        ("\x1b[5G", (4, 0)),
        ("\x1b[G", (0, 0)),
        ("\x1b[0G", (0, 0)),
        ("\x1b[999G", (19, 0)),
        ("\x1b[3`", (2, 0)),
        ("\x1b[3d", (0, 2)),
        ("\x1b[99d", (0, 4)),
    ];
    for (seq, expected) in cases {
        let mut vt = VtScreen::new(20, 5);
        feed(&mut vt, seq);
        assert_eq!(cursor(&vt), *expected, "{seq:?}");
    }
}

#[test]
fn cnl_cpl_move_lines_and_reset_column() {
    let cases: &[(&str, (usize, usize))] = &[
        ("\x1b[2E", (0, 4)),
        ("\x1b[9E", (0, 4)),
        ("\x1b[2F", (0, 0)),
        ("\x1b[1F", (0, 1)),
    ];
    for (seq, expected) in cases {
        let mut vt = VtScreen::new(20, 5);
        feed(&mut vt, "\x1b[3;4H");
        feed(&mut vt, seq);
        assert_eq!(cursor(&vt), *expected, "{seq:?}");
    }
}

#[test]
fn hpr_moves_cursor_right_and_clamps() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "ab\x1b[3a");
    assert_eq!(cursor(&vt), (5, 0));

    feed(&mut vt, "\x1b[99a");
    assert_eq!(cursor(&vt), (19, 0));
}

#[test]
fn cbt_moves_to_previous_tab_stop() {
    let cases: &[(&str, (usize, usize))] = &[
        ("\x1b[12G\x1b[Z", (8, 0)),
        ("\x1b[12G\x1b[2Z", (0, 0)),
        ("\x1b[9G\x1b[Z", (0, 0)),
        ("\x1b[1G\x1b[Z", (0, 0)),
    ];
    for (seq, expected) in cases {
        let mut vt = VtScreen::new(20, 5);
        feed(&mut vt, seq);
        assert_eq!(cursor(&vt), *expected, "{seq:?}");
    }
}

#[test]
fn ech_blanks_chars_without_moving_cursor() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "abcdef\x1b[3G\x1b[2X");
    assert_eq!(row(&vt, 0), "ab  ef");
    assert_eq!(cursor(&vt), (2, 0));
}

#[test]
fn ech_stops_at_end_of_line() {
    let mut vt = VtScreen::new(6, 2);
    feed(&mut vt, "abcdef\x1b[5G\x1b[9X");
    assert_eq!(row(&vt, 0), "abcd");
}

#[test]
fn ich_inserts_blanks_and_shifts_right() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "abcdef\x1b[3G\x1b[2@");
    assert_eq!(row(&vt, 0), "ab  cdef");
    assert_eq!(cursor(&vt), (2, 0));
}

#[test]
fn ich_drops_chars_pushed_past_right_edge() {
    let mut vt = VtScreen::new(6, 2);
    feed(&mut vt, "abcdef\x1b[2G\x1b[2@");
    assert_eq!(row(&vt, 0), "a  bcd");
}

#[test]
fn dch_deletes_chars_and_blank_fills_end() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "abcdef\x1b[3G\x1b[2P");
    assert_eq!(row(&vt, 0), "abef");
    assert_eq!(cursor(&vt), (2, 0));
}

#[test]
fn rep_repeats_last_printed_char() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "x\x1b[3b");
    assert_eq!(row(&vt, 0), "xxxx");
    assert_eq!(cursor(&vt), (4, 0));
}

#[test]
fn rep_without_prior_char_prints_nothing() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[3b");
    assert_eq!(row(&vt, 0), "");
    assert_eq!(cursor(&vt), (0, 0));
}

#[test]
fn decsc_decrc_save_and_restore_cursor() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "ab\x1b7\x1b[3;5H\x1b8X");
    assert_eq!(row(&vt, 0), "abX");
    assert_eq!(cursor(&vt), (3, 0));
}

#[test]
fn ris_resets_screen_modes_title_and_style() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[?1h\x1b[?2004h\x1b]0;t\x07\x1b[31mabc\x1bc");
    assert_eq!(vt.modes(), TermModes::default());
    assert_eq!(vt.title(), None);
    assert_eq!(row(&vt, 0), "");
    assert_eq!(cursor(&vt), (0, 0));
    assert_eq!(vt.grid().current_style, CellStyle::default());
}

#[test]
fn ris_keeps_scrollback_history() {
    let mut vt = VtScreen::new(10, 2);
    feed(&mut vt, "one\r\ntwo\r\nthree\x1bc");
    assert_eq!(vt.scrollback_text(None), "one");
}

#[test]
fn cursor_down_stops_at_scroll_bottom_when_starting_above_it() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r\x1b[10B");
    assert_eq!(cursor(&vt), (0, 3));
}

#[test]
fn cursor_down_below_region_clamps_to_last_row() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r\x1b[5;1H\x1b[10B");
    assert_eq!(cursor(&vt), (0, 4));
}

#[test]
fn cursor_up_stops_at_scroll_top_when_starting_inside_region() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r\x1b[3;1H\x1b[10A");
    assert_eq!(cursor(&vt), (0, 1));
}

#[test]
fn cursor_up_from_below_region_stops_at_scroll_top() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r\x1b[5;1H\x1b[10A");
    assert_eq!(cursor(&vt), (0, 1));
}

#[test]
fn cursor_up_above_region_clamps_to_first_row() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[2;4r\x1b[10A");
    assert_eq!(cursor(&vt), (0, 0));
}

#[test]
fn accessing_workspace_fragment_renders_at_column_one() {
    let mut vt = VtScreen::new(30, 2);
    feed(&mut vt, "\x1b[1GAccessing\x1b[11Gworkspace:");
    assert_eq!(row(&vt, 0), "Accessing workspace:");
}

#[test]
fn accessing_workspace_fragment_places_text_at_absolute_columns() {
    let mut vt = VtScreen::new(30, 2);
    feed(&mut vt, "\x1b[2GAccessing\x1b[12Gworkspace:");
    assert_eq!(vt.grid().get_cell(1, 0).unwrap().c, 'A');
    assert_eq!(vt.grid().get_cell(11, 0).unwrap().c, 'w');
    assert_eq!(row(&vt, 0), " Accessing workspace:");
}

fn bg_at(vt: &VtScreen, x: usize, y: usize) -> Option<Color> {
    vt.grid().get_cell(x, y).and_then(|c| c.style.bg)
}

#[test]
fn erasing_paints_the_current_background_but_no_attributes() {
    let mut vt = VtScreen::new(20, 4);
    feed(&mut vt, "\x1b[1;44mtab\x1b[K");
    let erased = vt.grid().get_cell(10, 0).copied().expect("cell");
    assert_eq!(erased.c, ' ');
    assert_eq!(
        erased.style,
        CellStyle {
            bg: Some(Color::Blue),
            ..CellStyle::default()
        },
        "only the background carries over"
    );
    assert_eq!(
        bg_at(&vt, 19, 0),
        Some(Color::Blue),
        "to the end of the line"
    );
    assert_eq!(bg_at(&vt, 0, 1), None, "other lines untouched");

    feed(&mut vt, "\x1b[0m\r\x1b[K");
    assert_eq!(
        bg_at(&vt, 10, 0),
        None,
        "after a reset, erase is default again"
    );
}

#[test]
fn cleared_screens_and_scrolled_in_lines_take_the_current_background() {
    let mut vt = VtScreen::new(10, 3);
    feed(&mut vt, "\x1b[41m\x1b[2J");
    assert!((0..3).all(|y| bg_at(&vt, 9, y) == Some(Color::Red)));

    feed(&mut vt, "\x1b[42m\x1b[3;1H\n");
    assert_eq!(bg_at(&vt, 5, 2), Some(Color::Green), "the new bottom line");
    assert_eq!(
        bg_at(&vt, 5, 1),
        Some(Color::Red),
        "the line above moved up"
    );
}

#[test]
fn reverse_video_is_set_by_sgr_7_and_cleared_by_27_or_reset() {
    let mut vt = VtScreen::new(10, 1);
    feed(&mut vt, "\x1b[7ma\x1b[27mb\x1b[7mc\x1b[0md");
    let reverse: Vec<bool> = (0..4)
        .map(|x| vt.grid().get_cell(x, 0).expect("cell").style.reverse)
        .collect();
    assert_eq!(reverse, [true, false, true, false]);
}

#[test]
fn private_csi_forms_do_not_act_as_sgr_or_cursor_restore() {
    let mut vt = VtScreen::new(20, 5);
    feed(&mut vt, "\x1b[3;4H\x1b[s\x1b[1;1H");
    feed(&mut vt, "\x1b[>4;2m\x1b[>1u\x1b[<u\x1b[?6nX");
    let cell = vt.grid().get_cell(0, 0).unwrap();
    assert_eq!(cell.c, 'X');
    assert!(!cell.style.underline);
    assert_eq!(cursor(&vt), (1, 0));
    assert!(vt.take_responses().is_empty());
}

#[test]
fn underline_sgr_honours_subparameters_and_ignores_underline_colour_arguments() {
    let mut vt = VtScreen::new(20, 5);
    feed(
        &mut vt,
        "\x1b[4:3mA\x1b[4:0mB\x1b[58;2;4;4;4mC\x1b[58;5;4mD\x1b[24m",
    );
    let underlined = |x| vt.grid().get_cell(x, 0).unwrap().style.underline;
    assert!(underlined(0));
    assert!(!underlined(1));
    assert!(!underlined(2));
    assert!(!underlined(3));
}

/// A 6x4 screen whose rows read `abcdef`, `ghijkl`, `mnopqr`, `stuvwx`.
fn lettered() -> VtScreen {
    let mut vt = VtScreen::new(6, 4);
    feed(&mut vt, "abcdef\r\nghijkl\r\nmnopqr\r\nstuvwx");
    vt
}

#[test]
fn csi_s_saves_the_cursor_unless_left_right_margin_mode_is_on() {
    let mut vt = lettered();
    feed(&mut vt, "\x1b[2;3H\x1b[s\x1b[H\x1b[u");
    assert_eq!(cursor(&vt), (2, 1), "CSI s / CSI u save and restore");
    feed(&mut vt, "\x1b[?69h\x1b[2;4s\x1b[S");
    assert_eq!(row(&vt, 0), "ahijef", "with DECLRMM, CSI s set margins 2..4");
}

#[test]
fn scrolling_inside_left_right_margins_moves_only_those_columns() {
    let mut vt = lettered();
    feed(&mut vt, "\x1b[?69h\x1b[2;3r\x1b[3;5s\x1b[S");
    assert_eq!(
        (row(&vt, 0), row(&vt, 1), row(&vt, 2), row(&vt, 3)),
        ("abcdef".into(), "ghopql".into(), "mn   r".into(), "stuvwx".into())
    );
    feed(&mut vt, "\x1b[T");
    assert_eq!((row(&vt, 1), row(&vt, 2)), ("gh   l".into(), "mnopqr".into()));
    assert_eq!(vt.grid().history_len(), 0, "a partial scroll keeps nothing");
}

#[test]
fn leaving_margin_mode_restores_full_width_scrolling() {
    let mut vt = lettered();
    feed(&mut vt, "\x1b[?69h\x1b[3;5s\x1b[?69l\x1b[S");
    assert_eq!(row(&vt, 0), "ghijkl");
    assert_eq!(vt.grid().history_len(), 1);
}

#[test]
fn delete_and_insert_line_respect_left_right_margins() {
    let mut vt = lettered();
    feed(&mut vt, "\x1b[?69h\x1b[1;2s\x1b[2;1H\x1b[M");
    assert_eq!((row(&vt, 1), row(&vt, 3)), ("mnijkl".into(), "  uvwx".into()));
    feed(&mut vt, "\x1b[2;1H\x1b[L");
    assert_eq!(row(&vt, 1), "  ijkl");
}
