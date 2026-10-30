//! Prefix bindings that split, open, rename, kill, move and resize panes and windows.
//! Panes spawn `ECHO` (never the user's shell) through config bindings.
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::tui::app::{Flow, Mode};

use mpx::layout::Direction;

use super::support::{ECHO, Harness, MAIN, SESSION, SIZE, connect, split, wait_for, window};

/// Default bindings that would spawn the user's shell, rebound to run `ECHO`.
fn echo_config() -> Config {
    let echo = |cmd: &str| format!("{cmd} sh -c '{ECHO}'");
    let mut config = Config::default();
    config.bind.insert("\"".into(), echo("split-window"));
    config.bind.insert("%".into(), echo("split-window -h"));
    config.bind.insert("c".into(), echo("new-window -n work"));
    config
}

async fn start() -> Harness {
    Harness::start(echo_config(), ECHO).await
}

/// Width of `pane` in the last prepared frame.
fn width_of(h: &Harness, pane: &str) -> u16 {
    h.rect_of(pane).width
}

#[tokio::test]
async fn split_and_window_bindings_create_panes_and_windows() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    assert_eq!(h.layout_panes().await, [MAIN, "pane-2"]);
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2", "new split takes focus");

    h.prefixed_char('"').await;
    assert_eq!(h.layout_panes().await.len(), 3);

    h.prefixed_char('c').await;
    assert_eq!(h.windows().await.len(), 2);
    let drawn = h.frame().await;
    assert!(drawn.contains("1:"), "status row lists the new window");
    assert_eq!(h.app.active_pane, "work");
}

#[tokio::test]
async fn rename_window_through_the_command_prompt() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(',').await;
    // `,` starts with the current name, which the user replaces.
    h.ctrl('u').await;
    h.type_str("logs").await;
    h.key(KeyCode::Enter).await;
    assert_eq!(h.windows().await, ["logs"]);
    assert!(h.frame().await.contains("[test] 0:logs*"));
}

#[tokio::test]
async fn kill_pane_asks_first_and_only_y_kills() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('"').await;
    h.frame().await;

    h.prefixed_char('x').await;
    assert!(h.frame().await.contains("kill-pane pane-2? (y/n)"));
    h.key(KeyCode::Char('n')).await;
    assert_eq!(h.layout_panes().await.len(), 2, "n keeps the pane");

    h.prefixed_char('x').await;
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.layout_panes().await, [MAIN]);
}

#[tokio::test]
async fn kill_window_asks_first_and_y_kills_the_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('c').await;
    h.frame().await;
    h.prefixed_char('&').await;
    assert!(h.frame().await.contains("kill-window? (y/n)"));
    h.key(KeyCode::Char('y')).await;
    assert_eq!(h.windows().await, [MAIN]);
}

#[tokio::test]
async fn swap_pane_exchanges_neighbours_in_the_layout() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.layout_panes().await, [MAIN, "pane-2"]);

    h.prefixed_char('{').await;
    assert_eq!(h.layout_panes().await, ["pane-2", MAIN]);
}

#[tokio::test]
async fn mark_pane_and_swap_with_prefix_bindings() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.layout_panes().await, [MAIN, "pane-2"]);
    // pane-2 is active. Mark it with prefix + m.
    h.prefixed_char('m').await;
    h.frame().await;
    assert!(h.app.layout.tabs[0].has_marked);
    assert_eq!(h.app.layout.tabs[0].marked_pane.as_deref(), Some("pane-2"));
    assert!(h.app.marked_rect.is_some());

    // Move focus to MAIN (left pane)
    h.prefixed_char('h').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);

    // Run swap-pane from prompt to swap active (MAIN) with marked (pane-2)
    h.prefixed_char(':').await;
    h.type_str("swap-pane").await;
    h.key(KeyCode::Enter).await;
    h.frame().await;
    assert_eq!(h.layout_panes().await, ["pane-2", MAIN]);

    // Clear mark with prefix + M
    h.prefixed_char('M').await;
    h.frame().await;
    assert!(!h.app.layout.tabs[0].has_marked);
    assert!(h.app.marked_rect.is_none());
}

#[tokio::test]
async fn break_pane_moves_the_pane_into_its_own_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    h.prefixed_char('!').await;
    assert_eq!(h.windows().await.len(), 2);
    assert!(h.nodes().await.contains(&"pane-2".to_string()));
}

#[tokio::test]
async fn zoom_shows_only_the_focused_pane_until_toggled_back() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.panes.len(), 2);

    h.prefixed_char('z').await;
    let screen = h.frame().await;
    assert!(h.app.zoomed());
    assert_eq!(h.app.panes.len(), 1);
    assert_eq!(h.app.panes[0].0, "pane-2");
    assert_eq!(
        h.app.panes[0].1.width, SIZE.0,
        "the zoomed pane fills the width"
    );
    assert!(
        screen.contains("*Z"),
        "the window tab shows the zoom: {screen}"
    );

    h.prefixed_char('z').await;
    let screen = h.frame().await;
    assert!(!h.app.zoomed());
    assert_eq!(h.app.panes.len(), 2);
    assert!(!screen.contains("*Z"));
}

#[tokio::test]
async fn zoom_set_on_the_daemon_by_another_client_is_drawn() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    h.probe.zoom(None, None).await.expect("zoom on the daemon");
    let screen = h.frame().await;
    assert!(h.app.zoomed());
    assert_eq!(h.app.panes.len(), 1);
    assert!(screen.contains("*Z"));

    h.probe.zoom(None, Some(false)).await.expect("unzoom");
    h.frame().await;
    assert!(!h.app.zoomed());
    assert_eq!(h.app.panes.len(), 2);
}

#[tokio::test]
async fn resize_pane_z_through_the_shim_toggles_the_zoom() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    let target = mpx::tmux_shim::EnvTarget {
        endpoint: h.sandbox.endpoint(SESSION),
        session: SESSION.to_string(),
    };
    mpx::tmux_shim::run_in(&target, "resize-pane -Z")
        .await
        .expect("resize-pane -Z");
    h.frame().await;
    assert!(h.app.zoomed());
    assert_eq!(h.app.panes.len(), 1);

    mpx::tmux_shim::run_in(&target, "resize-pane -Z")
        .await
        .expect("resize-pane -Z again");
    h.frame().await;
    assert!(!h.app.zoomed());
}

#[tokio::test]
async fn resize_moves_the_split_and_repeats_inside_the_window() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    let before = width_of(&h, MAIN);

    h.prefixed_char('H').await;
    h.frame().await;
    let after_prefix = width_of(&h, MAIN);
    assert!(after_prefix < before, "prefix H shrinks the left pane");

    h.key(KeyCode::Char('H')).await;
    h.frame().await;
    assert!(
        width_of(&h, MAIN) < after_prefix,
        "a bare H inside the repeat window resizes again"
    );
}

#[tokio::test]
async fn directional_bindings_move_focus_between_panes() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char('%').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");

    h.prefixed_char('h').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);

    h.prefixed_char('l').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "pane-2");
}

#[tokio::test]
async fn detach_binding_quits_the_app() {
    let mut h = start().await;
    h.frame().await;
    assert_eq!(h.prefixed_char('d').await, Flow::Quit);
    assert!(matches!(h.app.mode, Mode::Normal));
}

#[tokio::test]
async fn a_hidden_windows_output_never_reaches_the_tui_and_shows_once_selected() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    h.frame().await;
    seed.select_window(0).await.expect("back to main");
    h.frame().await;
    assert_eq!(h.app.active_pane, MAIN);
    while let Ok(item) = h.rx.try_recv() {
        h.app.on_stream(Some(item), &mut h.rx).await;
    }
    h.app.frame_due = None;

    h.probe.send_input("work", "BEHIND-THE-SCENES").await.expect("send");
    wait_for(&mut h.probe, "work", "BEHIND-THE-SCENES").await;
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(150), h.rx.recv()).await;
    if let Ok(Some(item)) = quiet {
        h.app.on_stream(Some(item), &mut h.rx).await;
    }
    let parsed = h
        .app
        .vt
        .get("work")
        .is_some_and(|vt| vt.snapshot().contains("BEHIND-THE-SCENES"));
    assert!(!parsed, "the daemon does not send a hidden window's output");
    assert_eq!(h.app.frame_due, None, "nothing on screen changed");

    h.prefixed_char('1').await;
    let shown = h.frame().await;
    assert!(shown.contains("BEHIND-THE-SCENES"), "rebuilt from the log: {shown}");
}

/// Applies stream events until `name` has exited, then prepares a frame; false means nothing is
/// left to show and the TUI ends.
async fn frame_after_exit(h: &mut Harness, name: &str) -> bool {
    h.probe.kill(name).await.expect("kill");
    let gone = name.to_string();
    h.until("exit delivered", move |app| !app.vt.contains_key(&gone))
        .await;
    h.app.layout.dirty = true;
    h.app.prepare_frame().await
}

#[tokio::test]
async fn the_shown_windows_last_pane_exiting_moves_on_to_another_window() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    window(&mut seed, "work", ECHO).await;
    seed.select_window(0).await.expect("back to main");
    h.frame().await;
    h.probe.send_input("work", "hidden").await.expect("send");
    wait_for(&mut h.probe, "work", "hidden").await;

    assert!(frame_after_exit(&mut h, MAIN).await, "another window still has a pane");
    assert!(h.frame().await.contains("hidden"));
}

#[tokio::test]
async fn the_sessions_last_pane_exiting_ends_the_tui() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    assert!(!frame_after_exit(&mut h, MAIN).await);
}

#[tokio::test]
async fn a_window_that_flooded_while_hidden_is_rebuilt_when_selected() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let mut seed = connect(&h.sandbox.endpoint(SESSION)).await;
    let script = "printf READY; read go; seq -f 'line %04.0f ------------------------------------------------------------' 1 8000; echo BIG-END; exec cat";
    window(&mut seed, "bulk", script).await;
    h.frame().await;
    seed.select_window(0).await.expect("back to main");
    h.frame().await;

    h.probe.send_input("bulk", "\n").await.expect("trigger");
    let log = wait_for(&mut h.probe, "bulk", "BIG-END").await;
    assert!(log.len() > 512 * 1024, "more than a catch-up's worth: {}", log.len());

    h.prefixed_char('1').await;
    let shown = h.frame().await;
    assert!(shown.contains("8000") && shown.contains("BIG-END"), "{shown}");
}

#[tokio::test]
async fn panes_split_or_focused_from_outside_are_laid_out_without_waiting_for_a_tick() {
    let mut h = start().await;
    h.frame().await;
    h.app.layout.dirty = false;
    split(&mut h.probe, MAIN, "agent", Direction::Horizontal).await;
    h.until("the split is laid out", |app| app.layout.dirty).await;

    h.frame().await;
    h.app.layout.dirty = false;
    h.probe.focus("agent").await.expect("focus");
    h.until("the focus change is laid out", |app| app.layout.dirty)
        .await;
}

#[tokio::test]
async fn dragging_a_border_with_the_mouse_resizes_the_panes_beside_it() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut h = start().await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    let before = width_of(&h, MAIN);
    let border = h.rect_of(MAIN).x + before;
    h.mouse(MouseEventKind::Down(MouseButton::Left), border, 3).await;
    h.frame().await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), border + 3, 3).await;
    h.frame().await;
    h.mouse(MouseEventKind::Drag(MouseButton::Left), border + 6, 4).await;
    h.frame().await;
    let height = h.rect_of(MAIN).height;
    let size = h.probe.status(MAIN).await.expect("status").size;
    assert_eq!(size, Some((before, height)), "not while dragging");
    h.mouse(MouseEventKind::Up(MouseButton::Left), border + 6, 4).await;
    h.frame().await;
    let width = width_of(&h, MAIN);
    assert!(width.abs_diff(before + 6) <= 1, "{before} -> {width}");
    let size = h.probe.status(MAIN).await.expect("status").size;
    assert_eq!(size, Some((width, height)), "on release");
    assert!(h.mode_is_normal(), "a border drag selects nothing");
}

#[tokio::test]
async fn focusing_a_pane_resized_elsewhere_gives_it_back_its_size() {
    let mut h = start().await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    let rect = h.rect_of("side");
    h.probe.resize_pane("side", 30, 8).await.expect("resize elsewhere");
    h.prefixed_char('o').await;
    h.frame().await;
    assert_eq!(h.app.active_pane, "side");
    let size = h.probe.status("side").await.expect("status").size;
    assert_eq!(size, Some((rect.width, rect.height)));
}

#[tokio::test]
async fn prefix_space_cycles_layouts_and_prefix_e_spreads_panes() {
    let mut h = start().await;
    split(&mut h.probe, MAIN, "side", Direction::Horizontal).await;
    h.frame().await;
    for _ in 0..2 {
        h.prefixed(KeyCode::Char(' ')).await;
        h.frame().await;
    }
    assert_eq!(h.rect_of(MAIN).x, h.rect_of("side").x, "stacked after two cycles");

    h.probe
        .resize_split(MAIN, mpx::protocol::Side::Down, 8, h.app.pane_viewport())
        .await
        .expect("uneven");
    h.frame().await;
    assert!(h.rect_of(MAIN).height > h.rect_of("side").height + 1);
    h.prefixed_char('E').await;
    h.frame().await;
    assert!(h.rect_of(MAIN).height.abs_diff(h.rect_of("side").height) <= 1);
}
