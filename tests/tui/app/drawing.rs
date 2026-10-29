//! What one frame shows: status row, exit badge, cursor placement and the bottom-row modes.
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::layout::Direction;
use mpx::protocol::{NewWindowOpts, SpawnOpts};
use mpx::tui::app::{FRAME_BUDGET, FRAMES_PER_BUDGET, needs_redraw, next_frame};

use super::support::{ECHO, Harness, MAIN, SESSION, connect, exited, pane, split, window};

async fn start() -> Harness {
    Harness::start(Config::default(), ECHO).await
}

#[tokio::test]
async fn status_row_shows_session_and_window_names() {
    let mut h = start().await;
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    let drawn = h.frame().await;
    assert!(
        drawn.contains("[test] 0:main-"),
        "session name leads the tabs"
    );
    assert!(
        drawn.contains("1:work*"),
        "tabs list windows, active one starred"
    );
}

#[tokio::test]
async fn exit_badge_marks_a_pane_whose_process_ended() {
    let mut h = start().await;
    let spawn = SpawnOpts {
        remain_on_exit: Some(true),
        ..pane("done", "printf READY; exit 3")
    };
    h.probe
        .new_window(NewWindowOpts {
            name: Some("done".into()),
            spawn,
            focus: true,
        })
        .await
        .expect("new window");
    assert_eq!(exited(&mut h.probe, "done").await, Some(3));
    h.app.refresh_nodes().await;
    assert!(h.frame().await.contains("[exited 3]"));
}

#[tokio::test]
async fn a_pane_that_reuses_an_exited_panes_name_gets_no_exit_badge() {
    let mut h = start().await;
    let opts = |script: &str| NewWindowOpts {
        name: Some("again".into()),
        spawn: pane("again", script),
        focus: true,
    };
    h.probe
        .new_window(opts("printf READY; exit 1"))
        .await
        .expect("first window");
    assert_eq!(exited(&mut h.probe, "again").await, Some(1));
    window(&mut h.probe, "again", ECHO).await;
    h.app.refresh_nodes().await;
    let drawn = h.frame().await;
    assert!(!drawn.contains("[exited"), "{drawn}");
}

#[tokio::test]
async fn cursor_is_placed_at_the_active_panes_cursor() {
    let mut h = start().await;
    h.frame().await;
    h.until("cursor after READY", |app| {
        app.active_vt().is_some_and(|vt| vt.grid().cursor.x == 5)
    })
    .await;
    h.frame().await;
    let cursor = h.cursor();
    assert_eq!((cursor.x, cursor.y), (5, 0), "cursor at column 6, row 1");
    assert!(cursor.visible, "cursor shown");
}

#[tokio::test]
async fn command_prompt_row_shows_text_and_places_cursor_on_it() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("ab").await;
    let drawn = h.frame().await;
    assert!(drawn.contains(":ab"));
    let cursor = h.cursor();
    assert_eq!(
        (cursor.x, cursor.y),
        (3, 29),
        "cursor after two characters on the last row"
    );
}

#[tokio::test]
async fn confirmation_question_replaces_the_status_row() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('x').await;
    assert!(h.frame().await.contains("kill-pane main? (y/n)"));
    h.key(KeyCode::Char('n')).await;
    assert!(!h.frame().await.contains("kill-pane main?"));
}

#[tokio::test]
async fn prefix_shows_the_shortcut_menu_after_the_delay_and_help_hides_the_cursor() {
    let mut h = start().await;
    h.app.config.which_key_delay_ms = 50;
    h.frame().await;
    h.ctrl('b').await;
    let early = h.frame().await;
    assert!(
        !early.contains("new window"),
        "the menu waits for the delay"
    );
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    assert!(h.frame().await.contains("new window"));

    h.key(KeyCode::Char('?')).await;
    h.frame().await;
    assert!(!h.cursor().visible, "overlay hides the cursor");
    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());
}

#[test]
fn output_after_a_quiet_spell_draws_at_once() {
    let now = Instant::now();
    assert_eq!(next_frame(None, now, None), now);
    let long_ago = now - Duration::from_secs(1);
    assert_eq!(next_frame(Some(long_ago), now, None), now);
}

#[test]
fn output_once_the_budget_is_spent_waits_for_its_window_to_pass() {
    let now = Instant::now();
    assert_eq!(next_frame(Some(now), now, None), now + FRAME_BUDGET);
}

#[tokio::test]
async fn a_burst_split_across_several_frames_is_not_held_back() {
    let mut h = start().await;
    h.frame().await;
    for _ in 0..FRAMES_PER_BUDGET - 1 {
        h.app.frame_due = Some(Instant::now());
        h.app.drew(Instant::now());
    }
    let before = h.app.expected.get(MAIN).copied().unwrap_or(0);
    h.probe.send_input(MAIN, "rest").await.expect("send");
    h.until("output applied", move |app| {
        app.expected.get(MAIN).copied().unwrap_or(0) > before
    })
    .await;
    let due = h.app.frame_due.expect("a frame is scheduled");
    assert!(
        due <= Instant::now(),
        "the last piece of a burst is not held back"
    );
}

#[test]
fn output_while_a_frame_is_pending_keeps_that_frame() {
    let now = Instant::now();
    let pending = now + Duration::from_millis(1);
    assert_eq!(next_frame(Some(now), now, Some(pending)), pending);
}

#[tokio::test]
async fn a_redraw_without_new_output_does_not_delay_the_next_output_frame() {
    let mut h = start().await;
    h.frame().await;
    h.app.drew(Instant::now());
    let before = h.app.expected.get(MAIN).copied().unwrap_or(0);
    h.probe.send_input(MAIN, "echo").await.expect("send");
    h.until("output applied", move |app| {
        app.expected.get(MAIN).copied().unwrap_or(0) > before
    })
    .await;
    let due = h.app.frame_due.expect("a frame is scheduled");
    assert!(
        due <= Instant::now(),
        "output after a keypress redraw is drawn at once, not a budget later"
    );
}

#[tokio::test]
async fn pane_output_schedules_a_frame_without_holding_up_input() {
    let mut h = start().await;
    h.frame().await;
    for _ in 0..FRAMES_PER_BUDGET {
        h.app.frame_due = Some(Instant::now());
        h.app.drew(Instant::now());
    }
    let before = h.app.expected.get(MAIN).copied().unwrap_or(0);
    h.probe.send_input(MAIN, "one").await.expect("send");
    let took = Instant::now();
    h.until("output applied", move |app| {
        app.expected.get(MAIN).copied().unwrap_or(0) > before
    })
    .await;
    assert!(
        took.elapsed() < FRAME_BUDGET,
        "applying output must not wait for the frame budget"
    );
    let due = h.app.frame_due.expect("a frame is scheduled");
    assert!(due > Instant::now(), "a spent budget holds the next output frame back");
    h.app.drew(Instant::now());
    assert_eq!(h.app.frame_due, None);
}

/// Bytes of the next frame. Only its size is asserted: frames are incremental, so their content is
/// checked through the harness terminal elsewhere.
async fn frame_bytes(h: &mut Harness) -> usize {
    h.app.layout.dirty = true;
    assert!(h.app.prepare_frame().await);
    let mut out = Vec::new();
    mpx::tui::frame::draw(&mut h.app, &mut out).expect("draw");
    h.terminal.feed(&out);
    out.len()
}

async fn emit_line(h: &mut Harness, text: &str) {
    let marker = text.to_string();
    h.probe.send_input(MAIN, &format!("{text}\r\n")).await.expect("send");
    h.until(text, move |app| {
        app.vt
            .get(MAIN)
            .is_some_and(|vt| vt.snapshot().contains(&marker))
    })
    .await;
}

#[tokio::test]
async fn a_scrolled_full_width_pane_sends_only_the_new_line() {
    let mut h = start().await;
    h.frame().await;
    for i in 0..40 {
        emit_line(&mut h, &format!("filler line {i:02} with some width to it")).await;
    }
    let repaint = frame_bytes(&mut h).await;
    emit_line(&mut h, "one more line").await;
    let scrolled = frame_bytes(&mut h).await;
    assert!(
        scrolled * 5 < repaint,
        "a one-line scroll sent {scrolled} bytes; a full frame is {repaint}"
    );
}

#[test]
fn input_redraws_at_once_unless_it_only_went_to_a_pane() {
    let now = Instant::now();
    assert!(needs_redraw(false, false, None, now));
    assert!(!needs_redraw(false, true, None, now), "the pane's answer brings its own frame");
}

#[test]
fn output_redraws_only_once_its_frame_is_due() {
    let now = Instant::now();
    assert!(needs_redraw(true, false, Some(now), now));
    assert!(!needs_redraw(true, false, Some(now + FRAME_BUDGET), now));
    assert!(
        needs_redraw(true, true, Some(now), now),
        "a due frame is drawn even right after a forwarded key"
    );
}

#[tokio::test]
async fn a_key_typed_into_a_pane_is_marked_as_forwarded_and_others_are_not() {
    let mut h = start().await;
    h.frame().await;
    h.key(KeyCode::Char('x')).await;
    assert!(std::mem::take(&mut h.app.forwarded));
    h.prefixed_char('?').await;
    assert!(!h.app.forwarded, "opening help changes the screen");
}

/// Draws enough output frames that the budget is spent for the next one.
fn spend_budget(h: &mut Harness) {
    for _ in 0..FRAMES_PER_BUDGET {
        h.app.frame_due = Some(Instant::now());
        h.app.drew(Instant::now());
    }
}

async fn output_from(h: &mut Harness, pane: &str, text: &str) -> Instant {
    let before = h.app.expected.get(pane).copied().unwrap_or(0);
    h.probe.send_input(pane, text).await.expect("send");
    let name = pane.to_string();
    h.until("output applied", move |app| {
        app.expected.get(&name).copied().unwrap_or(0) > before
    })
    .await;
    h.app.frame_due.expect("a frame is scheduled")
}

#[tokio::test]
async fn the_focused_panes_answer_to_a_keypress_skips_a_spent_budget() {
    let mut h = start().await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    spend_budget(&mut h);
    h.key(KeyCode::Char('x')).await;
    let due = output_from(&mut h, MAIN, "echo").await;
    assert!(due <= Instant::now(), "the echo waited for the budget");
}

#[tokio::test]
async fn other_panes_output_after_a_keypress_keeps_the_budget() {
    let mut h = start().await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    h.key(KeyCode::Char('x')).await;
    output_from(&mut h, MAIN, "").await;
    spend_budget(&mut h);
    let due = output_from(&mut h, "side", "noise").await;
    assert!(due > Instant::now(), "a flood elsewhere must not ride on the keypress");
}

#[tokio::test]
async fn a_keypress_lets_only_a_few_frames_skip_the_budget() {
    let mut h = start().await;
    h.frame().await;
    h.key(KeyCode::Char('x')).await;
    h.app.key_frames = 0;
    spend_budget(&mut h);
    let due = output_from(&mut h, MAIN, "more").await;
    assert!(due > Instant::now(), "the keypress's allowance is used up");
}

#[tokio::test]
async fn beside_the_agents_rail_a_scroll_sends_only_the_new_line_through_margins() {
    let mut h = start().await;
    h.app.canvas.set_lr_margins(true);
    let working = mpx::client::AgentReportArgs {
        harness: Some("claude".to_string()),
        state: Some(mpx::protocol::AgentState::Working),
        ..Default::default()
    };
    h.probe.agent_report(MAIN, working).await.expect("report agent");
    h.app.refresh_sidebar().await;
    h.frame().await;
    assert!(h.rect_of(MAIN).x > 0, "the pane sits right of the rail");
    for i in 0..40 {
        emit_line(&mut h, &format!("filler line {i:02} with some width to it")).await;
    }
    let repaint = frame_bytes(&mut h).await;
    emit_line(&mut h, "one more line").await;
    let scrolled = frame_bytes(&mut h).await;
    assert!(
        scrolled * 5 < repaint,
        "a one-line scroll beside the rail sent {scrolled} bytes; a full frame is {repaint}"
    );
}

#[test]
fn a_run_of_queued_drags_is_handled_at_its_last_position() {
    use crossterm::event::{Event, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
    use mpx::tui::app::latest_drag;
    let mouse = |kind, column| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        })
    };
    let drag = |column| mouse(MouseEventKind::Drag(MouseButton::Left), column);
    let up = mouse(MouseEventKind::Up(MouseButton::Left), 9);
    let mut queue = vec![drag(5), drag(7), up.clone(), drag(8)].into_iter();
    assert_eq!(latest_drag(drag(3), || queue.next()), (drag(7), Some(up)));
    assert_eq!(queue.next(), Some(drag(8)), "events after the run stay queued");
    let key = Event::Key(KeyEvent::from(KeyCode::Char('x')));
    let mut untouched = vec![drag(1)].into_iter();
    assert_eq!(latest_drag(key.clone(), || untouched.next()), (key, None));
    assert_eq!(untouched.len(), 1, "a key takes nothing from the queue");
}
