//! Panes that change height keep their content anchored like a terminal's: a pane whose cursor
//! sat on its bottom row keeps it there, and the daemon's screen and the app's screen agree.
use mpx::config::Config;
use mpx::layout::Direction;

use super::support::{ECHO, Harness, MAIN, split};

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

async fn start() -> Harness {
    let config = Config {
        which_key_delay_ms: 0,
        ..Config::default()
    };
    Harness::start(config, ECHO).await
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
}

#[tokio::test]
async fn closing_the_lower_pane_keeps_the_upper_content_bottom_anchored() {
    let mut h = start().await;
    stacked_then_close_below(&mut h, 20).await;
    let drawn = h.frame().await;
    let rows = pane_rows(&h, MAIN);
    let last = rows.len() - 1;
    assert_eq!(
        rows[last], "PROMPT$",
        "the prompt is on the bottom row\n{drawn}"
    );
    assert_eq!(rows[last - 1], "top 20", "{rows:#?}");
    assert!(
        rows.iter().any(|row| row == "READYtop 01"),
        "the first line is kept, from history\n{rows:#?}"
    );
    assert!(
        rows[0].is_empty(),
        "the rows above the content are the pad\n{rows:#?}"
    );
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
