//! Panes that change height keep their content anchored like a terminal's: a pane whose cursor
//! sat on its bottom row keeps it there, and the daemon's screen, the app's screen and a fresh
//! attach agree.
use std::time::{Duration, Instant};

use mpx::config::Config;
use mpx::layout::Direction;

use super::support::{ECHO, Harness, MAIN, SESSION, split};

fn numbered(prefix: &str, n: usize) -> String {
    (1..=n).map(|i| format!("{prefix} {i:02}\r\n")).collect()
}

/// The text of each row of `pane` in the drawn terminal, top to bottom.
fn pane_rows(h: &Harness, pane: &str) -> Vec<String> {
    let rect = h.rect_of(pane);
    (0..rect.height)
        .map(|dy| {
            (0..rect.width)
                .map(|dx| {
                    h.terminal
                        .grid()
                        .get_cell((rect.x + dx) as usize, (rect.y + dy) as usize)
                        .map_or(' ', |cell| cell.c)
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Rows without the blank ones after the last content row.
fn without_trailing_blanks(mut rows: Vec<String>) -> Vec<String> {
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn config() -> Config {
    Config {
        which_key_delay_ms: 0,
        ..Config::default()
    }
}

async fn start() -> Harness {
    Harness::start(config(), ECHO).await
}

/// Feeds the app the output a pty sends after a resize (ConPTY repaints the pane) until its screen
/// agrees with the daemon's. The app takes the daemon's screen at the resize; the repaint reaches
/// it afterwards as a stream event.
async fn caught_up_with_the_daemon(h: &mut Harness) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let daemon = h.probe.screen(MAIN).await.expect("daemon screen");
        let daemon = without_trailing_blanks(daemon.lines().map(str::to_string).collect());
        h.frame().await;
        if without_trailing_blanks(pane_rows(h, MAIN)) == daemon || Instant::now() >= deadline {
            return;
        }
        if let Ok(Some(item)) = tokio::time::timeout(Duration::from_millis(100), h.rx.recv()).await
        {
            h.app.on_stream(Some(item), &mut h.rx).await;
        }
    }
}

/// Splits the main pane with a pane below it, prints `lines` numbered lines and then `PROMPT$`
/// in the main pane, and closes the pane below.
async fn stacked_then_close_below(h: &mut Harness, lines: usize) {
    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    h.frame().await;
    let fill = format!("{}PROMPT$", numbered("top", lines));
    h.emit(MAIN, &fill, "PROMPT$").await;
    h.frame().await;
    h.probe.kill("below").await.expect("kill below");
    h.frame().await;
    caught_up_with_the_daemon(h).await;
}

/// A unix pty sends a program nothing on resize, so the grow replays the pane's output at the old
/// size and pulls its history back: the prompt ends on the bottom row. ConPTY repaints a resized
/// pane itself (see the Windows test); the pull-back is proven without a pty in `tests/vt`.
#[cfg(unix)]
#[tokio::test]
async fn closing_the_lower_pane_keeps_the_upper_content_bottom_anchored() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    let drawn = h.frame().await;
    let rows = pane_rows(&h, MAIN);
    let last = rows.len() - 1;
    assert_eq!(rows[last], "PROMPT$", "the prompt is on the bottom row\n{drawn}\n{rows:#?}");
    assert_eq!(rows[last - 1], "top 20", "{rows:#?}");
    assert!(
        rows.iter().any(|row| row == "READYtop 01"),
        "the first line is kept, from history\n{rows:#?}"
    );
    assert!(rows[0].is_empty(), "the rows above the content are the pad\n{rows:#?}");
}

/// ConPTY repaints a pane from its top row after a resize and keeps no scrollback to pull rows
/// back from, so the repaint overwrites the rows the grow pulled back (CI run 41775175372:
/// `top 08..top 20` and `PROMPT$` at the top, then blank rows). The pane must show that repaint.
/// The replay of the same bytes without a pty is `a_grow_replays_the_conpty_repaint_it_was_sent`.
#[cfg(windows)]
#[tokio::test]
async fn closing_the_lower_pane_shows_the_repaint_conpty_draws() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    let drawn = h.frame().await;
    let rows = pane_rows(&h, MAIN);
    let mut expected: Vec<String> = (8..=20).map(|i| format!("top {i:02}")).collect();
    expected.push("PROMPT$".to_string());
    let facts = h.facts(MAIN).await;
    assert_eq!(rows[..14], expected[..], "{drawn}\n{facts}");
    assert!(rows[14..].iter().all(String::is_empty), "{rows:#?}\n{facts}");
}

#[tokio::test]
async fn the_daemon_screen_and_the_app_screen_agree_after_the_close() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    let daemon = h.probe.screen(MAIN).await.expect("daemon screen");
    let daemon: Vec<String> = daemon.lines().map(str::to_string).collect();
    let app = without_trailing_blanks(pane_rows(&h, MAIN));
    assert_eq!(without_trailing_blanks(daemon), app);
}

#[tokio::test]
async fn a_pane_whose_cursor_is_above_its_bottom_row_keeps_its_rows_on_top() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 3).await;
    let rows = pane_rows(&h, MAIN);
    assert!(rows[0].starts_with("READY"), "{rows:#?}");
    assert_eq!(rows[3], "PROMPT$", "{rows:#?}");
    assert!(
        rows[4..].iter().all(String::is_empty),
        "blank below the prompt\n{rows:#?}"
    );
}

#[tokio::test]
async fn a_fresh_attach_after_the_close_shows_what_the_live_pane_shows() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    let live = pane_rows(&h, MAIN);
    let Harness { sandbox, .. } = h;
    let mut fresh = Harness::attach(sandbox, SESSION, config()).await;
    fresh.frame().await;
    assert_eq!(pane_rows(&fresh, MAIN), live);
}

#[tokio::test]
async fn a_fresh_attach_shows_a_repaint_written_after_the_close() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    // A full-screen repaint at the new size, as a program redraws after SIGWINCH (or ConPTY does).
    let repaint: String = std::iter::once("\x1b[H".to_string())
        .chain((1..=29).map(|row| format!("\x1b[{row};1Hrow {row:02}")))
        .collect();
    h.emit(MAIN, &repaint, "row 29").await;
    h.frame().await;
    let live = pane_rows(&h, MAIN);
    let Harness { sandbox, .. } = h;
    let mut fresh = Harness::attach(sandbox, SESSION, config()).await;
    fresh.frame().await;
    assert_eq!(pane_rows(&fresh, MAIN), live, "live\n{live:#?}");
}
