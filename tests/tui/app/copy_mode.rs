//! Copy mode (prefix `[`, wheel, mouse drag) and mouse focus, through real input events.
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::layout::Direction;
use mpx::tui::app::{App, Mode};

use super::support::{ECHO, Harness, MAIN, long_output, split, two_lines, wait_for};

/// Whether the main pane shows the end of the fill: `line 35`, and `READY` on the last row. The
/// `READY` the pane printed at start is on screen too, so `READY` alone does not show the fill.
fn fill_applied(app: &App) -> bool {
    app.active_vt().is_some_and(|vt| {
        let shown = vt.snapshot();
        shown.contains("line 35") && shown.trim_end().ends_with("READY")
    })
}

#[tokio::test]
async fn prefix_bracket_enters_copy_mode_and_q_leaves_it() {
    let mut h = Harness::started(ECHO).await;
    h.prefixed_char('[').await;
    assert!(h.frame().await.contains("COPY -0/0"));
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
    assert!(!h.frame().await.contains("COPY"));
}

#[tokio::test]
async fn keyboard_selection_yanks_the_selected_text() {
    let mut h = Harness::started(&two_lines()).await;
    h.prefixed_char('[').await;
    h.key(KeyCode::Char('0')).await;
    h.key(KeyCode::Char('v')).await;
    for _ in 0..4 {
        h.key(KeyCode::Char('l')).await;
    }
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.clipboard(), ["READY"]);
}

#[tokio::test]
async fn mouse_drag_selects_and_release_copies() {
    let mut h = Harness::started(&two_lines()).await;
    h.frame().await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 0).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0).await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), 4, 0).await;
    assert_eq!(h.clipboard(), ["hello"]);
    assert!(h.mode_is_normal(), "a copy by drag leaves copy mode");
}

#[tokio::test]
async fn a_drag_on_a_clicked_pane_leaves_it_streaming_after_release() {
    let mut h = Harness::started(ECHO).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    let target = h.rect_of("plain");
    let (col, row) = (target.x + 1, target.y + 1);
    h.mouse(MouseEventKind::Down(MouseButton::Left), col, row)
        .await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), col + 1, row)
        .await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), col + 1, row)
        .await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(h.mode_is_normal());

    h.frame().await;
    h.emit("plain", "LATE-OUTPUT\r", "LATE-OUTPUT").await;
    assert!(h.frame().await.contains("LATE-OUTPUT"));
}

#[tokio::test]
async fn mouse_wheel_scrolls_into_history() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;

    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(
        h.frame().await.contains("COPY -0/0"),
        "copy mode opens before its history"
    );
    h.history_arrives().await;
    assert!(h.frame().await.contains("COPY -3/"));
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(h.frame().await.contains("COPY -6/"));
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(h.frame().await.contains("COPY -3/"));
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(
        h.mode_is_normal(),
        "back at the live screen, copy mode ends"
    );
    assert!(!h.frame().await.contains("COPY"));
}

#[tokio::test]
async fn copy_mode_entered_from_the_keyboard_outlasts_the_wheel_and_keys_at_the_bottom() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.prefixed_char('[').await;
    h.ctrl('u').await;
    h.ctrl('d').await;
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "keys back at the bottom"
    );
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(matches!(h.app.mode, Mode::Copy(_)), "only q or Esc leave");
}

#[tokio::test]
async fn mouse_click_focuses_the_clicked_pane_without_copy_mode() {
    let mut h = Harness::started(ECHO).await;
    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    let target = h.rect_of("plain");
    assert_eq!(h.app.active_pane, MAIN);

    let (col, row) = (target.x + 1, target.y + 1);
    h.mouse(MouseEventKind::Down(MouseButton::Left), col, row)
        .await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), col, row)
        .await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(h.mode_is_normal(), "a click only focuses");
    assert!(h.clipboard().is_empty());
}

#[tokio::test]
async fn leaving_copy_mode_shows_output_that_arrived_meanwhile_without_scrollback() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.history_arrives().await;
    assert!(h.frame().await.contains("COPY -3/"));

    h.probe
        .send_input(MAIN, "MISSED-WHILE-SCROLLED\r")
        .await
        .unwrap();
    wait_for(&mut h.probe, MAIN, "MISSED-WHILE-SCROLLED").await;
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(h.mode_is_normal());
    let frame = h.frame().await;
    assert!(frame.contains("MISSED-WHILE-SCROLLED"), "{frame}");
    assert!(!frame.contains("COPY"));
    assert_eq!(
        h.app.vt[MAIN].grid().history_len(),
        0,
        "a live pane keeps no scrollback"
    );
}

#[tokio::test]
async fn leaving_a_pane_in_copy_mode_keeps_its_view_and_returning_resumes_it() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.history_arrives().await;
    assert!(h.frame().await.contains("COPY -6/"));

    split(&mut h.probe, MAIN, "plain", Direction::Horizontal).await;
    h.frame().await;
    h.prefixed_char('o').await;
    let frame = h.frame().await;
    assert_eq!(h.app.active_pane, "plain");
    assert!(h.mode_is_normal(), "the other pane is not in copy mode");
    assert!(frame.contains("COPY -6/"), "the left pane keeps its view");

    h.prefixed_char('o').await;
    assert!(h.frame().await.contains("COPY -6/"));
    assert_eq!(h.app.active_pane, MAIN);
    assert!(matches!(h.app.mode, Mode::Copy(_)));
}

#[tokio::test]
async fn the_prefix_alone_does_not_end_copy_mode() {
    let mut h = Harness::started(ECHO).await;
    h.prefixed_char('[').await;
    h.ctrl('b').await;
    h.key(KeyCode::Esc).await;
    assert!(h.frame().await.contains("COPY -0/0"));
}

#[tokio::test]
async fn clicking_a_pane_in_copy_mode_keeps_its_scroll_and_selects_nothing() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.history_arrives().await;
    assert!(h.frame().await.contains("COPY -3/"));

    h.mouse(MouseEventKind::Down(MouseButton::Left), 2, 2).await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), 2, 2).await;
    let frame = h.frame().await;
    assert!(frame.contains("COPY -3/"), "{frame}");
    assert!(h.clipboard().is_empty());

    h.mouse(MouseEventKind::Down(MouseButton::Left), 2, 2).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 6, 2).await;
    h.mouse(MouseEventKind::Up(MouseButton::Left), 6, 2).await;
    assert!(h.frame().await.contains("VISUAL -3/"));
    assert_eq!(h.clipboard().len(), 1);
}

#[tokio::test]
async fn copy_mode_prefix_opens_the_prefix_menu() {
    let mut h = Harness::started(ECHO).await;
    h.app.config.which_key_delay_ms = 0;
    h.prefixed_char('[').await;
    h.ctrl('b').await;
    assert!(h.frame().await.contains("new window"));
}

#[tokio::test]
async fn copy_mode_search_up_and_down_moves_cursor_and_scrolls() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.prefixed_char('[').await;
    assert!(h.frame().await.contains("COPY -0/"));

    // Press '?' to start search up
    h.key(KeyCode::Char('?')).await;
    let prompt_frame = h.frame().await;
    assert!(prompt_frame.contains('?'), "shows ? prompt on status row");

    // Type "line 5" and press Enter
    for c in "line 5".chars() {
        h.key(KeyCode::Char(c)).await;
    }
    h.key(KeyCode::Enter).await;

    let match_frame = h.frame().await;
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "still in copy mode after search"
    );
    if let Mode::Copy(cs) = &h.app.mode {
        assert!(
            cs.scroll_offset > 0,
            "scrolled back to match: {match_frame}"
        );
        assert_eq!(cs.cursor_x, 0, "cursor at start of 'line 5'");
    }

    // Press 'n' to find next match
    h.key(KeyCode::Char('n')).await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));

    // Press 'q' to leave copy mode
    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
}

fn history_of(h: &Harness) -> usize {
    h.app.active_vt().map_or(0, |vt| vt.grid().history_len())
}

#[tokio::test]
async fn a_live_pane_keeps_no_scrollback_and_copy_mode_brings_it_back() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    assert_eq!(
        history_of(&h),
        0,
        "the daemon keeps scrollback, not the live view"
    );

    h.prefixed_char('[').await;
    assert!(
        history_of(&h) >= 10,
        "copy mode rebuilds it from the pane's log"
    );
    let shown = h.frame().await;
    assert!(shown.contains("COPY -0/"), "{shown}");
    assert!(shown.contains("line 40"), "the view is unchanged: {shown}");

    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
    assert_eq!(history_of(&h), 0, "leaving copy mode drops it again");
    assert!(h.frame().await.contains("line 40"));
}

#[tokio::test]
async fn a_drag_selection_does_not_rebuild_scrollback() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 5).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 5, 5).await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));
    assert_eq!(history_of(&h), 0, "selecting on screen needs no history");
    h.mouse(MouseEventKind::Up(MouseButton::Left), 5, 5).await;
    assert_eq!(h.clipboard().len(), 1);
    assert!(h.mode_is_normal());
}

#[tokio::test]
async fn wheeling_during_a_drag_loads_history_and_keeps_the_anchor() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 5).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 5, 5).await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(history_of(&h) >= 10, "the wheel rebuilds the scrollback");
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 5, 0).await;
    let Mode::Copy(cs) = &h.app.mode else {
        panic!("still in copy mode");
    };
    let grid = h.app.active_vt().expect("vt").grid();
    assert_eq!(
        cs.anchor_abs_y,
        grid.abs_line(5, 0),
        "anchor stays on the pressed row"
    );
    h.mouse(MouseEventKind::Up(MouseButton::Left), 5, 0).await;
    assert_eq!(h.clipboard().len(), 1);
}

/// Ticks until the focused pane's history has been built ahead of copy mode.
async fn warm_up(h: &mut Harness) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !h.app.scrollback_warm() {
        assert!(
            std::time::Instant::now() < deadline,
            "history was never built"
        );
        h.app.on_tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn copy_mode_takes_the_history_built_ahead_and_shows_output_since() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    assert!(!h.app.scrollback_warm(), "nothing is built before a tick");
    warm_up(&mut h).await;
    assert_eq!(history_of(&h), 0, "the live pane still keeps no scrollback");

    h.probe.send_input(MAIN, "AFTER-WARM\r").await.unwrap();
    wait_for(&mut h.probe, MAIN, "AFTER-WARM").await;
    h.prefixed_char('[').await;
    assert!(
        !h.app.scrollback_warm(),
        "copy mode used up the built history"
    );
    assert!(history_of(&h) >= 10, "the history came with it");
    let frame = h.frame().await;
    assert!(frame.contains("COPY -0/"), "{frame}");
    assert!(
        frame.contains("AFTER-WARM"),
        "output since the build is fed: {frame}"
    );

    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal());
    assert_eq!(history_of(&h), 0, "leaving copy mode drops it again");
}

#[tokio::test]
async fn the_wheel_takes_the_history_built_ahead() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    warm_up(&mut h).await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(!h.app.scrollback_warm());
    assert!(h.frame().await.contains("COPY -3/"));
}

#[tokio::test]
async fn wheeling_back_down_before_the_history_arrives_leaves_copy_mode_and_drops_it() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "one notch is still owed"
    );
    h.mouse(MouseEventKind::ScrollDown, 5, 5).await;
    assert!(h.mode_is_normal());

    h.history_arrives().await;
    assert!(
        h.mode_is_normal(),
        "a history for a closed copy mode is dropped"
    );
    assert_eq!(history_of(&h), 0);
}

#[tokio::test]
async fn a_selection_started_before_the_history_arrives_keeps_its_anchor() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    h.mouse(MouseEventKind::Down(MouseButton::Left), 0, 5).await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), 5, 5).await;
    h.history_arrives().await;

    let Mode::Copy(cs) = &h.app.mode else {
        panic!("still in copy mode");
    };
    let grid = h.app.active_vt().expect("vt").grid();
    assert_eq!(
        cs.anchor_abs_y,
        grid.abs_line(5, 0),
        "the anchor stays on the pressed row"
    );
}

#[tokio::test]
async fn a_history_gone_stale_stays_usable_while_a_newer_one_builds() {
    let script = r"for i in $(seq 1 40); do printf 'line %s\r\n' $i; done;
        stty raw -echo; printf READY; read go;
        yes xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx | head -n 9000; echo FLOODED; exec cat";
    let mut h = Harness::started(script).await;
    h.frame().await;
    warm_up(&mut h).await;

    h.probe.send_input(MAIN, "go\n").await.unwrap();
    h.until("flood applied", |app| {
        app.active_vt()
            .is_some_and(|vt| vt.snapshot().contains("FLOODED"))
    })
    .await;

    h.app.on_tick().await;
    assert!(
        h.app.scrollback_warm(),
        "the stale history is still there to use"
    );
    h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    assert!(history_of(&h) >= 10);
    assert!(h.frame().await.contains("COPY -3/"));
}

/// Applies the stream events that arrive while `quiet` passes without a new one.
async fn drain_stream(h: &mut Harness) {
    let quiet = std::time::Duration::from_millis(300);
    while let Ok(Some(item)) = tokio::time::timeout(quiet, h.rx.recv()).await {
        h.app.on_stream(Some(item), &mut h.rx).await;
    }
}

#[tokio::test]
async fn output_scrolling_the_pane_in_copy_mode_leaves_the_view_and_selection_alone() {
    let mut h = Harness::started(ECHO).await;
    let mut fill = String::new();
    for i in 1..=35 {
        fill.push_str(&format!("line {i}\r\n"));
    }
    fill.push_str("READY");
    h.probe.send_input(MAIN, &fill).await.expect("send");
    h.until("fill lines applied", fill_applied).await;
    h.frame().await;
    h.prefixed_char('[').await;
    let initial_hist = history_of(&h);
    assert!(initial_hist >= 5);

    h.key(KeyCode::Char('0')).await;
    h.key(KeyCode::Char('v')).await;
    for _ in 0..4 {
        h.key(KeyCode::Char('l')).await;
    }
    let frame_before = h.frame().await;
    assert!(frame_before.contains("VISUAL -0/"));

    h.probe
        .send_input(MAIN, "\r\nmore 1\r\nmore 2")
        .await
        .expect("send");
    wait_for(&mut h.probe, MAIN, "more 2").await;
    drain_stream(&mut h).await;

    let frame = h.frame().await;
    assert_eq!(history_of(&h), initial_hist, "{frame}");
    assert!(frame.contains("VISUAL -0/"), "badge: {frame}");
    assert!(!frame.contains("more"), "the view is frozen: {frame}");
    assert!(
        frame.contains("READY"),
        "viewport still shows READY: {frame}"
    );

    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.clipboard(), ["READY"], "selection stays on READY");
    assert!(h.mode_is_normal());
    let live_frame = h.frame().await;
    assert!(
        live_frame.contains("more 2"),
        "leaving copy mode shows the output that arrived meanwhile: {live_frame}"
    );
}

#[tokio::test]
async fn output_redrawing_the_live_rows_does_not_change_the_copy_view() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.prefixed_char('[').await;
    let before = h.frame().await;
    assert!(before.contains("READY"), "{before}");

    h.probe
        .send_input(MAIN, "\x1b[H\x1b[2JREDRAWN")
        .await
        .expect("send");
    wait_for(&mut h.probe, MAIN, "REDRAWN").await;
    drain_stream(&mut h).await;

    let frame = h.frame().await;
    assert!(
        frame.contains("READY") && !frame.contains("REDRAWN"),
        "{frame}"
    );

    h.key(KeyCode::Char('q')).await;
    let live = h.frame().await;
    assert!(
        live.contains("REDRAWN") && !live.contains("READY"),
        "{live}"
    );
}

/// Runs `line` as typed at the `:` prompt, as `copy_commands` does.
async fn run_line_as_typed(h: &mut Harness, line: &str) {
    let cmd = mpx::tui::command::parse(line).expect("a command");
    h.app.run(cmd).await;
}

/// Splits the main pane with a pane below it, then enters copy mode in the main pane at the top of
/// its history.
async fn copy_at_top_of_split_main(h: &mut Harness) {
    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    h.frame().await;
    h.prefixed_char('[').await;
    for _ in 0..30 {
        h.mouse(MouseEventKind::ScrollUp, 5, 5).await;
    }
}

#[tokio::test]
async fn a_copy_pane_that_grows_stays_at_the_top_of_its_history() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    copy_at_top_of_split_main(&mut h).await;
    let before = history_of(&h);
    let top = h.frame().await;
    assert!(top.contains(&format!("COPY -{before}/{before}")), "{top}");

    h.probe.kill("below").await.expect("kill below");
    let frame = h.frame().await;
    let after = history_of(&h);
    assert!(
        frame.contains(&format!("COPY -{after}/{after}")),
        "{frame}\n{}",
        h.facts(MAIN).await
    );
    assert!(
        frame.starts_with("line 1 "),
        "the view is at the top of history\n{frame}"
    );
    // Only a unix pty leaves a grow to the replay: ConPTY repaints the screen on a resize and
    // its repaint scrolls the rows back into history. The pull-back is proven in tests/vt.
    #[cfg(unix)]
    assert!(
        after < before,
        "growing pulls history back (before {before}, after {after})"
    );
}

#[tokio::test]
async fn a_mark_in_copy_mode_still_points_at_its_line_after_a_resize() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    copy_at_top_of_split_main(&mut h).await;
    run_line_as_typed(&mut h, "send-keys -X set-mark").await;
    h.probe.kill("below").await.expect("kill below");
    h.frame().await;

    let Mode::Copy(cs) = &h.app.mode else {
        panic!("the pane is still in copy mode");
    };
    let grid = h.app.active_vt().expect("the pane's screen").grid();
    let marked = cs
        .mark
        .and_then(|row| grid.abs_at(row))
        .and_then(|line| grid.line_text(line));
    assert_eq!(marked.as_deref().map(str::trim_end), Some("line 1"));
}

#[tokio::test]
async fn a_mark_is_dropped_when_a_resize_reflows_the_copy_pane() {
    let mut h = Harness::started(&long_output()).await;
    h.frame().await;
    h.prefixed_char('[').await;
    run_line_as_typed(&mut h, "send-keys -X set-mark").await;
    assert!(matches!(&h.app.mode, Mode::Copy(cs) if cs.mark.is_some()));

    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    assert!(
        matches!(&h.app.mode, Mode::Copy(cs) if cs.mark.is_none()),
        "a row number no longer names its line once the lines reflow"
    );
}
