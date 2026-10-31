//! Render fidelity: every frame the app draws lands on the harness terminal, which keeps its state
//! across frames, and after each frame every visible pane's region must match that pane's own
//! screen. This catches stale cells when frames are drawn incrementally, including the vim-style
//! sequences that broke rendering before (scroll regions, insert/delete line, alternate screen).
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::layout::Direction;
use mpx::layout::Rect;
use mpx::tui::app::ClockSource;
use mpx::tui::labels::{label, label_rect, pane_numbers};

use super::support::{ECHO, Harness, MAIN, split};

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
                    assert_eq!(got.style, want.style, "{step}: pane {name} style at ({x},{y})");
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

/// The cells the display-panes numbers cover in the last frame.
fn number_areas(h: &Harness) -> Vec<Rect> {
    let mut areas = Vec::new();
    for (number, name) in pane_numbers(&h.app.panes, &h.app.nodes) {
        let found = h.app.panes.iter().find(|(pane, ..)| *pane == name);
        let Some(&(_, area, _)) = found else { continue };
        let block = label(&number.to_string(), area.width as usize, area.height as usize);
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

    h.emit(MAIN, "\x1b[2J\x1b[Hcleared\x1b[5;5HCLEAR-DONE", "CLEAR-DONE").await;
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
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "help overlay closed").await;

    h.ctrl('b').await;
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "prefix HUD dismissed").await;

    h.key(KeyCode::Char(':')).await;
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "command prompt cancelled").await;

    h.emit(MAIN, "\x1b[H\x1b[2Jfinal\r\nFINAL-DONE", "FINAL-DONE").await;
    check(&mut h, "redraw after overlays").await;


    h.prefixed_char('?').await;
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "overlay closed").await;
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
    h.probe.zoom(None, None).await.expect("zoom from another client");
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
    h.app.clock = ClockSource::Fixed(9, 41);
    h.frame().await;
    let clock = h.rect_of(MAIN);
    check_masked(&mut h, "clock fills the active pane", &[clock]).await;
    h.key(KeyCode::Char('x')).await;
    check(&mut h, "clock dismissed").await;
}
