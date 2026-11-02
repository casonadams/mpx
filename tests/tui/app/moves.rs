//! `join-pane`, `swap-window`, `respawn-pane` and `find-window` at the `:` prompt.
use mpx::tui::app::Mode;

use super::support::{Harness, MAIN};

#[tokio::test]
async fn join_pane_moves_the_marked_pane_next_to_the_active_one() {
    let mut h = Harness::idle().await;
    h.open_window("other").await;
    h.frame().await;
    h.app.refresh_nodes().await;
    assert_eq!(h.windows().await.len(), 2);

    h.command(&format!("select-pane -m -t {MAIN}")).await;
    h.frame().await;
    h.command("join-pane -h").await;

    assert_eq!(h.windows().await, ["other"], "the emptied window went away");
    let mut panes = h.layout_panes().await;
    panes.sort();
    assert_eq!(panes, [MAIN, "other"]);
}

#[tokio::test]
async fn join_pane_without_a_mark_says_so() {
    let mut h = Harness::idle().await;
    let said = h.command_message("join-pane").await.unwrap_or_default();
    assert_eq!(said, "no marked pane");
}

#[tokio::test]
async fn swap_window_exchanges_the_active_window_with_the_target() {
    let mut h = Harness::idle().await;
    h.open_window("other").await;
    h.frame().await;
    h.app.refresh_nodes().await;
    let before = h.windows().await;
    assert_eq!(before.len(), 2);

    h.command("swap-window -t 0").await;
    let after = h.windows().await;
    assert_eq!(after, [before[1].clone(), before[0].clone()]);
}

#[tokio::test]
async fn respawn_pane_with_kill_shows_the_new_process_on_the_same_pane() {
    let mut h = Harness::idle().await;
    h.command("respawn-pane -k sh -c 'printf AGAIN; exec cat'").await;
    h.until("the old screen dropped", |app| !app.expected.contains_key(MAIN)).await;
    h.frame().await;
    h.until("the respawned output", |app| {
        app.vt.get(MAIN).is_some_and(|vt| vt.snapshot().contains("AGAIN"))
    })
    .await;
    assert_eq!(h.layout_panes().await, [MAIN]);
}

#[tokio::test]
async fn respawn_pane_of_a_running_pane_without_kill_is_refused() {
    let mut h = Harness::idle().await;
    let said = h.command_message("respawn-pane").await.unwrap_or_default();
    assert!(said.contains("still active"), "{said}");
}

#[tokio::test]
async fn find_window_opens_the_window_list_filtered_by_the_pattern() {
    let mut h = Harness::idle().await;
    h.command("find-window main").await;
    assert!(matches!(h.app.mode, Mode::Overlay(_)));
}
