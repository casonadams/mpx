//! Render fidelity: every frame the app draws lands on the harness terminal, which keeps its state
//! across frames, and after each frame every visible pane's region must match that pane's own
//! screen. This catches stale cells when frames are drawn incrementally, including the vim-style
//! sequences that broke rendering before (scroll regions, insert/delete line, alternate screen).
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::layout::Direction;

use super::support::{ECHO, Harness, MAIN, split};

/// Draws a frame and asserts that every visible pane shows exactly its own screen; the focused
/// (undimmed) pane must also match attribute for attribute.
async fn check(h: &mut Harness, step: &str) {
    h.frame().await;
    for (name, rect, focused) in h.app.panes.clone() {
        let pane = h.app.vt.get(&name).expect("pane screen");
        for y in 0..rect.height as usize {
            for x in 0..rect.width as usize {
                let want = pane.grid().get_cell(x, y).copied().unwrap_or_default();
                let got = h
                    .terminal
                    .grid()
                    .get_cell(rect.x as usize + x, rect.y as usize + y)
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

/// Sends `bytes` to `pane` (an `ECHO` pane prints them back) and waits until its screen shows `marker`.
async fn emit(h: &mut Harness, pane: &str, bytes: &str, marker: &str) {
    h.probe.send_input(pane, bytes).await.expect("send");
    let pane = pane.to_string();
    let wanted = marker.to_string();
    h.until(marker, move |app| {
        app.vt
            .get(&pane)
            .is_some_and(|vt| vt.snapshot().contains(&wanted))
    })
    .await;
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
    emit(&mut h, MAIN, &fill, "FILL-DONE").await;
    check(&mut h, "scrolling fill").await;

    emit(&mut h, MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "vim-style redraw on the alternate screen").await;

    emit(
        &mut h,
        MAIN,
        "\x1b[2J\x1b[Hcleared\x1b[5;5HCLEAR-DONE",
        "CLEAR-DONE",
    )
    .await;
    check(&mut h, "full clear").await;

    emit(&mut h, MAIN, "\x1b[?1049lALT-OFF", "ALT-OFF").await;
    check(&mut h, "alternate screen left").await;

    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    check(&mut h, "after split").await;
    let side = format!("{}SIDE-DONE", numbered_lines("side", 35));
    emit(&mut h, "side", &side, "SIDE-DONE").await;
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

    emit(
        &mut h,
        MAIN,
        "\x1b[H\x1b[2Jfinal\r\nFINAL-DONE",
        "FINAL-DONE",
    )
    .await;
    check(&mut h, "redraw after overlays").await;

    let working = mpx::client::AgentReportArgs {
        harness: Some("claude".to_string()),
        state: Some(mpx::protocol::AgentState::Working),
        ..Default::default()
    };
    h.probe.agent_report(MAIN, working).await.expect("report agent");
    h.app.refresh_sidebar().await;
    check(&mut h, "agents rail shown").await;
    let busy = format!("{}BUSY-DONE", numbered_lines("busy", 30));
    emit(&mut h, MAIN, &busy, "BUSY-DONE").await;
    check(&mut h, "output beside the rail").await;
    emit(&mut h, MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "vim-style redraw beside the rail").await;

    h.prefixed_char('e').await;
    h.frame().await;
    assert!(h.terminal.snapshot().starts_with(" Agents"), "rail expanded");
    let under = format!("{}UNDER-DONE", numbered_lines("under", 8));
    emit(&mut h, MAIN, &under, "UNDER-DONE").await;
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "output that arrived under the expanded rail").await;

    h.prefixed_char('?').await;
    h.frame().await;
    h.key(KeyCode::Esc).await;
    check(&mut h, "overlay closed over the rail").await;
    let dot = h.terminal.grid().get_cell(0, 0).map(|c| c.c);
    assert_eq!(dot, Some('●'), "rail repainted after the overlay");
    h.prefixed_char('z').await;
    check(&mut h, "zoomed beside the rail").await;
    h.prefixed_char('z').await;
    check(&mut h, "unzoomed beside the rail").await;

    h.prefixed_char('z').await;
    let zoomed = format!("{}ZOOM-DONE", numbered_lines("zoom", 12));
    emit(&mut h, MAIN, &zoomed, "ZOOM-DONE").await;
    check(&mut h, "zoomed pane scrolls on the terminal").await;
    h.prefixed_char('z').await;
    check(&mut h, "unzoomed after scrolling").await;

    h.probe.kill("side").await.expect("kill side");
    split(&mut h.probe, MAIN, "below", Direction::Vertical).await;
    check(&mut h, "stacked panes").await;
    let below = format!("{}BELOW-DONE", numbered_lines("below", 9));
    emit(&mut h, "below", &below, "BELOW-DONE").await;
    check(&mut h, "lower pane scrolls inside its own rows").await;
    let above = format!("{}ABOVE-DONE", numbered_lines("above", 9));
    emit(&mut h, MAIN, &above, "ABOVE-DONE").await;
    check(&mut h, "upper pane scrolls inside its own rows").await;
    emit(&mut h, MAIN, VIM_LIKE, "VIM-DONE").await;
    check(&mut h, "scroll regions inside a stacked pane").await;
}
