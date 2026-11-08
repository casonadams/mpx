//! The status line and message row as the frame draws them, with the options that move, restyle
//! or renumber them, and the mouse and copy-mode options that change what keys and clicks do.
use std::time::Duration;

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::config::{Config, ModeKeys, StatusPosition};
use mpx::tui::app::{Mode, sync_mouse_capture};

use super::support::{ECHO, Harness, SESSION, SIZE, active_window, connect, window};

async fn start(config: Config) -> Harness {
    Harness::start(config, ECHO).await
}

async fn start_hidden() -> Harness {
    start(Config {
        status: false,
        ..Config::default()
    })
    .await
}

/// Row `index` of a frame, empty when the frame has fewer rows.
fn row(drawn: &str, index: usize) -> String {
    drawn.lines().nth(index).unwrap_or_default().to_string()
}

fn bottom(drawn: &str) -> String {
    row(drawn, SIZE.1 as usize - 1)
}

/// Opens a second window `work`, which becomes the current one.
async fn with_work_window(h: &mut Harness) {
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    h.frame().await;
}

#[tokio::test]
async fn hidden_status_line_gives_the_bottom_row_to_the_panes() {
    let mut h = start_hidden().await;
    let drawn = h.frame().await;
    assert!(h.app.rows.status.is_none());
    assert_eq!(h.app.panes[0].1.height, SIZE.1, "the panes fill every row");
    assert!(
        !bottom(&drawn).contains("[test]"),
        "no status line: {:?}",
        bottom(&drawn)
    );
}

#[tokio::test]
async fn messages_still_show_on_the_bottom_row_when_the_status_line_is_hidden() {
    let mut h = start_hidden().await;
    h.frame().await;
    h.app.say("hello there");
    let drawn = h.frame().await;
    assert!(
        bottom(&drawn).contains("hello there"),
        "{:?}",
        bottom(&drawn)
    );
}

#[tokio::test]
async fn top_status_line_takes_the_first_row_and_panes_start_below_it() {
    let mut h = start(Config {
        status_position: StatusPosition::Top,
        ..Config::default()
    })
    .await;
    let drawn = h.frame().await;
    assert!(
        row(&drawn, 0).contains("[test] 0:main*"),
        "{:?}",
        row(&drawn, 0)
    );
    assert_eq!(
        h.app.panes[0].1.y, 1,
        "the pane starts under the status line"
    );
    assert_eq!(h.app.panes[0].1.height, SIZE.1 - 1);
}

#[tokio::test]
async fn custom_status_left_and_right_replace_the_defaults() {
    let mut h = start(Config {
        status_left: "<#{session_name}>".into(),
        status_right: "#[fg=red]right %Y".into(),
        ..Config::default()
    })
    .await;
    let drawn = h.frame().await;
    let bar = bottom(&drawn);
    assert!(bar.starts_with("<test>"), "{bar:?}");
    assert!(
        !bar.contains("[test]"),
        "the built-in label is gone: {bar:?}"
    );
    assert!(
        bar.trim_end().contains("right 20"),
        "strftime expands %Y: {bar:?}"
    );
    assert!(!bar.contains("%Y"));
}

#[tokio::test]
async fn default_status_bar_is_tmuxs_with_a_copy_badge() {
    let mut h = start(Config::default()).await;
    let bar = bottom(&h.frame().await);
    assert!(bar.starts_with("[test] 0:main*"), "{bar:?}");
    let right = bar
        .trim_end()
        .rsplit_once('"')
        .map(|(_, t)| t)
        .unwrap_or_default();
    let date = right.trim().rsplit(' ').next().unwrap_or_default();
    assert_eq!(date.len(), 9, "%d-%b-%y at the right edge: {bar:?}");
    assert!(!bar.contains("[copy]"), "{bar:?}");
    h.prefixed_char('[').await;
    let bar = bottom(&h.frame().await);
    assert!(bar.contains("[copy] "), "{bar:?}");
}

#[tokio::test]
async fn base_index_numbers_the_window_tabs_from_it() {
    let mut h = start(Config {
        base_index: 1,
        ..Config::default()
    })
    .await;
    with_work_window(&mut h).await;
    let drawn = h.frame().await;
    let bar = bottom(&drawn);
    assert!(
        bar.contains("1:main-") && bar.contains("2:work*"),
        "{bar:?}"
    );
}

#[tokio::test]
async fn digit_keys_select_the_window_with_that_number() {
    let mut h = start(Config {
        base_index: 1,
        ..Config::default()
    })
    .await;
    with_work_window(&mut h).await;
    h.prefixed_char('1').await;
    h.frame().await;
    assert_eq!(active_window(&h), "main", "digit 1 is the first window");
    h.prefixed_char('2').await;
    h.frame().await;
    assert_eq!(active_window(&h), "work", "digit 2 is the second window");
    h.prefixed_char('0').await;
    assert_eq!(
        h.app.live_message(),
        Some("can't find window: 0"),
        "no window 0 with base-index 1"
    );
}

#[tokio::test]
async fn zero_base_keeps_digit_zero_on_the_first_window() {
    let mut h = start(Config::default()).await;
    with_work_window(&mut h).await;
    h.prefixed_char('0').await;
    h.frame().await;
    assert_eq!(active_window(&h), "main");
}

/// The window current after clicking the `main` tab in the status line of a fresh session.
async fn after_clicking_main_tab(config: Config) -> String {
    let mut h = start(config).await;
    with_work_window(&mut h).await;
    // "[test] 0:main- " puts the main tab at column 7.
    h.mouse(MouseEventKind::Down(MouseButton::Left), 8, SIZE.1 - 1)
        .await;
    h.frame().await;
    active_window(&h)
}

#[tokio::test]
async fn clicking_a_status_tab_selects_its_window() {
    assert_eq!(after_clicking_main_tab(Config::default()).await, "main");
}

#[tokio::test]
async fn mouse_off_ignores_clicks() {
    let config = Config {
        mouse: false,
        ..Config::default()
    };
    assert_eq!(after_clicking_main_tab(config).await, "work");
}

#[tokio::test]
async fn display_time_sets_how_long_a_message_stays() {
    let mut h = start(Config {
        display_time_ms: 40,
        ..Config::default()
    })
    .await;
    h.app.say("hello");
    assert_eq!(h.app.live_message(), Some("hello"));
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(h.app.live_message(), None);
}

fn copy_cursor(h: &Harness) -> (usize, usize) {
    match &h.app.mode {
        Mode::Copy(cs) => (cs.cursor_x, cs.cursor_y),
        _ => panic!("not in copy mode"),
    }
}

#[tokio::test]
async fn emacs_mode_keys_drive_copy_mode() {
    let mut h = start(Config {
        mode_keys: ModeKeys::Emacs,
        ..Config::default()
    })
    .await;
    h.frame().await;
    h.prefixed_char('[').await;
    let start = copy_cursor(&h);
    h.key(KeyCode::Char('j')).await;
    assert_eq!(
        copy_cursor(&h),
        start,
        "vi motions are unbound under emacs keys"
    );
    h.ctrl('f').await;
    assert_eq!(copy_cursor(&h).0, start.0 + 1, "C-f moves right");
}

/// Runs `wanted` through `sync_mouse_capture` from a captured start, returning the captured
/// flag and the bytes written so far after each step.
fn mouse_capture_steps(wanted: &[bool]) -> Vec<(bool, usize)> {
    let mut out = Vec::new();
    let mut captured = true;
    wanted
        .iter()
        .map(|&want| {
            sync_mouse_capture(&mut out, want, &mut captured);
            (captured, out.len())
        })
        .collect()
}

const MOUSE_SEQUENCE: [bool; 4] = [true, false, false, true];

/// Mouse reporting is written as escape sequences on unix only. crossterm's Windows
/// `EnableMouseCapture`/`DisableMouseCapture` set the console mode through WinAPI and write no
/// bytes (`is_ansi_code_supported` is false there), so the byte checks cannot hold on Windows.
#[cfg(unix)]
#[test]
fn mouse_capture_is_written_only_when_it_changes() {
    let steps = mouse_capture_steps(&MOUSE_SEQUENCE);
    let written: Vec<usize> = steps.iter().map(|&(_, len)| len).collect();
    assert_eq!(written[0], 0, "already captured");
    assert!(
        written[1] > 0,
        "turning mouse off writes the terminal sequence"
    );
    assert_eq!(written[2], written[1], "no repeat when the state holds");
    assert!(
        written[3] > written[2],
        "turning mouse back on writes again"
    );
}

#[test]
fn mouse_capture_state_follows_every_change_on_every_platform() {
    let captured: Vec<bool> = mouse_capture_steps(&MOUSE_SEQUENCE)
        .into_iter()
        .map(|(captured, _)| captured)
        .collect();
    assert_eq!(captured, MOUSE_SEQUENCE);
}
