//! Render fidelity: every frame the app draws lands on the harness terminal, which keeps its state
//! across frames, and after each frame every visible pane's region must match that pane's own
//! screen. This catches stale cells when frames are drawn incrementally, including the vim-style
//! sequences that broke rendering before (scroll regions, insert/delete line, alternate screen).
use crossterm::event::KeyCode;

use mpx::clients::WindowSize;
use mpx::config::{Config, WindowOptions};
use mpx::layout::Direction;
use mpx::layout::Rect;
use mpx::tui::app::{AttachOpts, ClockSource, Mode};
use mpx::tui::labels::{label, label_rect, pane_numbers};

use super::support::{ECHO, Harness, MAIN, SESSION, SIZE, open_app, screen_text, split};

/// Whether the cell at (`x`, `y`) lies inside `area`.
fn inside(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
}

/// Draws a frame and asserts that every visible pane shows exactly its own screen, except the cells
/// inside `masked`, which an overlay covers on purpose. The focused (undimmed) pane must also match
/// attribute for attribute.
pub(super) async fn check_masked(h: &mut Harness, step: &str, masked: &[Rect]) {
    h.frame().await;
    for (name, rect, focused) in h.app.panes.clone() {
        let pane = h.app.vt.get(&name).expect("pane screen");
        for y in 0..rect.height as usize {
            for x in 0..rect.width as usize {
                let (tx, ty) = (rect.x + x as u16, rect.y + y as u16);
                if masked.iter().any(|area| inside(*area, tx, ty)) {
                    continue;
                }
                let want = pane.grid().get_cell(x, y).copied().unwrap_or_default();
                let got = h
                    .terminal
                    .grid()
                    .get_cell(tx as usize, ty as usize)
                    .copied()
                    .unwrap_or_default();
                assert_eq!(
                    got.c,
                    want.c,
                    "{step}: pane {name} char at ({x},{y})\nterminal:\n{}",
                    h.terminal.snapshot()
                );
                if focused {
                    assert_eq!(
                        got.style, want.style,
                        "{step}: pane {name} style at ({x},{y})"
                    );
                }
            }
        }
    }
}

/// Draws a frame and asserts that every visible pane shows exactly its own screen; the focused
/// (undimmed) pane must also match attribute for attribute.
async fn check(h: &mut Harness, step: &str) {
    check_masked(h, step, &[]).await;
}

/// Closes the overlay the caller just opened with Esc, then asserts every pane matches again.
async fn esc_overlay(h: &mut Harness, step: &str) {
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(h, step).await;
}

/// The cells the display-panes numbers cover in the last frame.
fn number_areas(h: &Harness) -> Vec<Rect> {
    let mut areas = Vec::new();
    for (number, name) in pane_numbers(&h.app.panes, &h.app.nodes) {
        let found = h.app.panes.iter().find(|(pane, ..)| *pane == name);
        let Some(&(_, area, _)) = found else { continue };
        let block = label(
            &number.to_string(),
            area.width as usize,
            area.height as usize,
        );
        areas.push(label_rect(area, &block));
    }
    areas
}

fn numbered_lines(prefix: &str, n: usize) -> String {
    (1..=n)
        .map(|i| {
            let accent = if i % 2 == 0 { " café → ✓" } else { "" };
            format!(
                "{prefix} line {i:02}{accent} \x1b[3{};1mcolored\x1b[0m \x1b[9{}mbright\x1b[0m\r\n",
                i % 8,
                i % 8
            )
        })
        .collect()
}

const VIM_LIKE: &str = concat!(
    "\x1b[?1049h\x1b[2J\x1b[H",
    "\x1b[44;3m status \x1b[0m \x1b[7m reverse fill \x1b[K\x1b[0m\r\n",
    "alpha\r\nbeta\r\ngamma\r\ndelta\r\nepsilon\r\nzeta\r\neta\r\ntheta\r\n",
    "\x1b[3;12r\x1b[12;1H",
    "\r\nscrolled-1\r\nscrolled-2\r\nscrolled-3",
    "\x1b[4;1H\x1b[2L",
    "\x1b[7;1H\x1b[3M",
    "\x1b[r\x1b[20;10H\x1b[4mpositioned\x1b[24m\x1b[20;30G\x1b[38;5;208mcha\x1b[38;2;10;20;30mrgb\x1b[0m",
    "\x1b[22;1H\x1b[KVIM-DONE"
);

#[tokio::test]
async fn frames_keep_the_terminal_identical_to_the_panes() {
    let config = Config {
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    // The harness terminal has left/right margins, so panes beside the sidebar or a split scroll
    // through them; the drawing tests cover terminals without.
    h.app.canvas.set_lr_margins(true);
    check(&mut h, "attach").await;

    let fill = format!("{}FILL-DONE", numbered_lines("main", 40));
    h.emit(MAIN, &fill, "FILL-DONE").await;
    check(&mut h, "scrolling fill").await;

    h.emit(MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "vim-style redraw on the alternate screen").await;

    h.emit(
        MAIN,
        "\x1b[2J\x1b[Hcleared\x1b[5;5HCLEAR-DONE",
        "CLEAR-DONE",
    )
    .await;
    check(&mut h, "full clear").await;

    h.emit(MAIN, "\x1b[?1049lALT-OFF", "ALT-OFF").await;
    check(&mut h, "alternate screen left").await;

    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    check(&mut h, "after split").await;
    let side = format!("{}SIDE-DONE", numbered_lines("side", 35));
    h.emit("side", &side, "SIDE-DONE").await;
    check(&mut h, "unfocused pane scrolls").await;

    h.prefixed_char('z').await;
    check(&mut h, "zoomed").await;
    h.prefixed_char('z').await;
    check(&mut h, "unzoomed").await;

    h.prefixed_char('?').await;
    esc_overlay(&mut h, "help overlay closed").await;

    h.ctrl('b').await;
    esc_overlay(&mut h, "prefix HUD dismissed").await;

    h.key(KeyCode::Char(':')).await;
    esc_overlay(&mut h, "command prompt cancelled").await;

    h.emit(MAIN, "\x1b[H\x1b[2Jfinal\r\nFINAL-DONE", "FINAL-DONE")
        .await;
    check(&mut h, "redraw after overlays").await;

    h.prefixed_char('?').await;
    esc_overlay(&mut h, "overlay closed").await;
    h.prefixed_char('z').await;
    check(&mut h, "zoomed pane").await;
    h.prefixed_char('z').await;
    check(&mut h, "unzoomed pane").await;

    h.prefixed_char('z').await;
    let zoomed = format!("{}ZOOM-DONE", numbered_lines("zoom", 12));
    h.emit(MAIN, &zoomed, "ZOOM-DONE").await;
    check(&mut h, "zoomed pane scrolls on the terminal").await;
    h.prefixed_char('z').await;
    check(&mut h, "unzoomed after scrolling").await;

    // Zoom set on the daemon by another client: the terminal follows it, scrolling included.
    h.probe
        .zoom(None, None)
        .await
        .expect("zoom from another client");
    check(&mut h, "zoomed by another client").await;
    let scrolled = format!("{}DAEMON-ZOOM-DONE", numbered_lines("daemon", 12));
    h.emit(MAIN, &scrolled, "DAEMON-ZOOM-DONE").await;
    check(&mut h, "zoomed by another client scrolls").await;
    h.probe
        .zoom(None, Some(false))
        .await
        .expect("unzoom from another client");
    check(&mut h, "unzoomed by another client").await;

    h.probe.kill("side").await.expect("kill side");
    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    check(&mut h, "stacked panes").await;
    let below = format!("{}BELOW-DONE", numbered_lines("below", 9));
    h.emit("below", &below, "BELOW-DONE").await;
    check(&mut h, "lower pane scrolls inside its own rows").await;
    let above = format!("{}ABOVE-DONE", numbered_lines("above", 9));
    h.emit(MAIN, &above, "ABOVE-DONE").await;
    check(&mut h, "upper pane scrolls inside its own rows").await;
    h.emit(MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "scroll regions inside a stacked pane").await;
}

#[tokio::test]
async fn pane_overlays_cover_only_their_own_cells() {
    let config = Config {
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    h.app.canvas.set_lr_margins(true);
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    check(&mut h, "after split").await;
    h.emit("side", "side text", "side text").await;
    check(&mut h, "two panes").await;

    h.command("display-panes -d 0").await;
    h.frame().await;
    let numbers = number_areas(&h);
    assert_eq!(numbers.len(), 2, "one number per pane");
    check_masked(&mut h, "pane numbers shown", &numbers).await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "pane numbers dismissed").await;

    h.command("clock-mode").await;
    h.app.chrome.clock = ClockSource::Fixed(9, 41);
    h.frame().await;
    let clock = h.rect_of(MAIN);
    check_masked(&mut h, "clock fills the active pane", &[clock]).await;
    h.key(KeyCode::Char('x')).await;
    check(&mut h, "clock dismissed").await;
}

#[tokio::test]
async fn two_status_lines_keep_each_pane_identical_to_its_screen() {
    let config = Config {
        which_key_delay_ms: 0,
        status_lines: 2,
        status_format: vec![String::new(), "second #{session_name}".to_string()],
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    h.app.canvas.set_lr_margins(true);
    check(&mut h, "attach with two status lines").await;
    assert_eq!(h.app.rows.status_h, 2);
    let drawn = h.frame().await;
    let second = drawn
        .lines()
        .nth(usize::from(SIZE.1) - 1)
        .unwrap_or_default();
    assert!(second.contains("second test"), "{second:?}");

    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    check(&mut h, "after split above two status lines").await;
    h.emit("side", "side text", "side text").await;
    check(&mut h, "two panes above two status lines").await;
}

/// A window laid out for a smaller client than this one leaves the rest of this client's pane area
/// empty: every cell no pane covers is blank, and the panes still show their own screens.
#[tokio::test]
async fn a_window_laid_out_smaller_than_this_client_blanks_the_rest() {
    let config = Config {
        window: WindowOptions {
            window_size: WindowSize::Smallest,
            ..WindowOptions::default()
        },
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    check(&mut h, "attach").await;
    let fill = format!("{}FILL-DONE", numbered_lines("main", 40));
    h.emit(MAIN, &fill, "FILL-DONE").await;
    check(&mut h, "full window").await;

    let small_config = Config {
        window: WindowOptions {
            window_size: WindowSize::Smallest,
            ..WindowOptions::default()
        },
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let (mut small, _events) = open_app(
        &h.sandbox,
        SESSION,
        small_config,
        AttachOpts::default(),
        Some("small".into()),
    )
    .await;
    small.report_size(60, 20).await;
    let _ = h.app.on_tick().await;
    check(&mut h, "window smaller than this client").await;

    let (w, height) = h.app.viewport;
    let top = h.app.rows.pane_top;
    let mut blank_cells = 0;
    for y in top..top + height {
        for x in 0..w {
            if h.app.panes.iter().any(|(_, r, _)| r.contains(x, y)) {
                continue;
            }
            let cell = h.terminal.grid().get_cell(x as usize, y as usize);
            assert_eq!(cell.map(|c| c.c), Some(' '), "uncovered cell ({x},{y})");
            blank_cells += 1;
        }
    }
    assert!(blank_cells > 0, "the smaller window leaves cells uncovered");
}

#[tokio::test]
async fn links_underlines_sync_and_overlays_keep_the_terminal_identical() {
    let config = Config {
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    h.app.canvas.set_lr_margins(true);
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    check(&mut h, "after split").await;

    // Hyperlinks (OSC 8), underline styles and underline colours reach the terminal as the pane
    // holds them.
    let styled = concat!(
        "\x1b]8;;https://fid.test/a\x1b\\LINK\x1b]8;;\x1b\\ ",
        "\x1b[4:3mcurly\x1b[0m \x1b[4:2mdouble\x1b[0m ",
        "\x1b[58;5;196m\x1b[4mred\x1b[0m \x1b[58;2;1;2;3m\x1b[4:5mcolour\x1b[0m STYLE-DONE",
    );
    h.emit(MAIN, styled, "STYLE-DONE").await;
    check(&mut h, "links and underline styles").await;

    // A synchronized update holds the pane's drawing: the frame keeps the last complete state
    // until the program ends the update.
    h.probe.send_input(MAIN, "\x1b[?2026hHELD-TEXT").await.expect("send");
    h.until("update begun", |app| app.vt.get(MAIN).is_some_and(|vt| vt.sync_output())).await;
    let held = h.frame().await;
    assert!(!held.contains("HELD-TEXT"), "a synchronized update is held\n{held}");
    h.emit(MAIN, "\x1b[?2026l SYNC-END", "SYNC-END").await;
    check(&mut h, "synchronized update ended").await;

    // A popup covers its box of the panes; the panes outside it still match their screens.
    h.command("display-popup -E -w 40 -h 10 'printf POPUP-OUT; read x'").await;
    h.until_popup("popup printed", |popup| screen_text(popup).contains("POPUP-OUT")).await;
    let popup = match &h.app.mode {
        Mode::Popup(popup) => popup.rect,
        _ => panic!("popup should be open"),
    };
    check_masked(&mut h, "popup overlay", &[popup]).await;
    h.key(KeyCode::Enter).await;
    h.until_popup("popup closed", |_| false).await;
    check(&mut h, "popup closed").await;

    // A menu covers its own box the same way.
    h.command("display-menu -x 5 -y 5 Zoom z 'resize-pane -Z'").await;
    let menu = match &h.app.mode {
        Mode::Menu(menu) => menu.rect,
        _ => panic!("menu should be open"),
    };
    check_masked(&mut h, "menu overlay", &[menu]).await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "menu closed").await;
}

/// A pane that grows when the pane below it closes keeps its terminal identical to its screen,
/// with its content bottom-anchored, on the main screen and on the alternate screen.
#[tokio::test]
async fn a_pane_that_grows_keeps_the_terminal_identical_to_its_screen() {
    let config = Config {
        which_key_delay_ms: 0,
        ..Config::default()
    };
    let mut h = Harness::start(config, ECHO).await;
    h.app.canvas.set_lr_margins(true);
    check(&mut h, "attach").await;

    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    check(&mut h, "stacked panes").await;
    let fill = format!("{}PROMPT$", numbered_lines("top", 20));
    h.emit(MAIN, &fill, "PROMPT$").await;
    check(&mut h, "upper pane filled").await;
    h.probe.kill("below").await.expect("kill below");
    check(&mut h, "upper pane grows with content").await;

    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    check(&mut h, "stacked again").await;
    h.emit(MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "alternate screen in the upper pane").await;
    h.probe.kill("below").await.expect("kill below again");
    check(&mut h, "alternate screen pane grows").await;
}

/// Unix only: ConPTY holds a synchronized update itself, so the text is not delivered during it.
#[cfg(unix)]
#[tokio::test]
async fn a_held_synchronized_update_has_a_deadline_that_ends_the_hold() {
    use std::time::Instant;
    use mpx::tui::outer::SYNC_HOLD;

    let mut h = Harness::started(ECHO).await;
    h.frame().await;
    assert!(h.app.sync_deadline().is_none(), "nothing is held");

    h.probe.send_input(MAIN, "\x1b[?2026hHELD-TEXT").await.expect("send");
    h.until("update begun", |app| app.vt.get(MAIN).is_some_and(|vt| vt.sync_output())).await;
    assert!(!h.frame().await.contains("HELD-TEXT"));
    let due = h.app.sync_deadline().expect("a held pane wakes the loop");
    assert!(due <= Instant::now() + SYNC_HOLD);

    let began = Instant::now().checked_sub(SYNC_HOLD * 2).expect("clock");
    h.app.outer.sync_since.insert(MAIN.to_string(), began);
    assert!(h.app.sync_deadline().is_none(), "an expired hold does not wake the loop again");
    assert!(h.frame().await.contains("HELD-TEXT"), "the expired hold draws the pane");
}
