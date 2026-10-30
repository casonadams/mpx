//! The status line and message row as the frame draws them, with the options that move, restyle
//! or renumber them, and the mouse and copy-mode options that change what keys and clicks do.
use std::time::Duration;

use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::config::{Config, ModeKeys, StatusPosition};
use mpx::protocol::VarKind;
use mpx::tui::app::{Mode, sync_mouse_capture};

use super::support::{ECHO, Harness, SESSION, SIZE, connect, window};

async fn start(config: Config) -> Harness {
    Harness::start(config, ECHO).await
}

/// Row `index` of a frame, empty when the frame has fewer rows.
fn row(drawn: &str, index: usize) -> String {
    drawn.lines().nth(index).unwrap_or_default().to_string()
}

fn bottom(drawn: &str) -> String {
    row(drawn, SIZE.1 as usize - 1)
}

/// Name of the window the app shows as current.
fn active_window(h: &Harness) -> String {
    h.app
        .layout
        .tabs
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.name.clone())
        .unwrap_or_default()
}

/// Opens a second window `work`, which becomes the current one.
async fn with_work_window(h: &mut Harness) {
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    h.frame().await;
}

#[tokio::test]
async fn hidden_status_line_gives_the_bottom_row_to_the_panes() {
    let mut h = start(Config {
        status: false,
        ..Config::default()
    })
    .await;
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
    let mut h = start(Config {
        status: false,
        ..Config::default()
    })
    .await;
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
async fn custom_status_left_and_right_replace_the_built_in_sides() {
    let mut h = start(Config {
        status_left: Some("<#{session_name}>".into()),
        status_right: Some("#[fg=red]right %Y".into()),
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
async fn set_option_status_off_takes_effect_on_the_next_frame() {
    let mut h = start(Config::default()).await;
    h.frame().await;
    h.probe
        .set_var(VarKind::Option, "status", Some("off"))
        .await
        .expect("set status");
    h.app.apply_runtime().await;
    let drawn = h.frame().await;
    assert!(!bottom(&drawn).contains("[test]"), "{:?}", bottom(&drawn));
    assert!(h.app.rows.status.is_none());
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

#[test]
fn mouse_capture_is_written_only_when_it_changes() {
    let mut out = Vec::new();
    let mut captured = true;
    sync_mouse_capture(&mut out, true, &mut captured);
    assert!(out.is_empty(), "already captured");
    sync_mouse_capture(&mut out, false, &mut captured);
    assert!(
        !out.is_empty(),
        "turning mouse off writes the terminal sequence"
    );
    assert!(!captured);
    let after_off = out.len();
    sync_mouse_capture(&mut out, false, &mut captured);
    assert_eq!(out.len(), after_off, "no repeat when the state holds");
    sync_mouse_capture(&mut out, true, &mut captured);
    assert!(
        captured && out.len() > after_off,
        "turning mouse back on writes again"
    );
}
