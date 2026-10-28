//! What one frame shows: status row, exit badge, cursor placement and the bottom-row modes.
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::protocol::{NewWindowOpts, SpawnOpts};

use super::support::{ECHO, Harness, SESSION, connect, exited, pane, window};

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
async fn cursor_is_placed_at_the_active_panes_cursor() {
    let mut h = start().await;
    h.frame().await;
    h.until("cursor after READY", |app| {
        app.active_vt().is_some_and(|vt| vt.grid().cursor.x == 5)
    })
    .await;
    let drawn = h.frame().await;
    assert!(drawn.contains("\x1b[1;6H"), "cursor at column 6, row 1");
    assert!(drawn.contains("\x1b[?25h"), "cursor shown");
}

#[tokio::test]
async fn command_prompt_row_shows_text_and_places_cursor_on_it() {
    let mut h = start().await;
    h.frame().await;
    h.prefixed_char(':').await;
    h.type_str("ab").await;
    let drawn = h.frame().await;
    assert!(drawn.contains(":ab"));
    assert!(
        drawn.contains("\x1b[30;4H"),
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
async fn prefix_shows_the_shortcut_menu_and_help_hides_the_cursor() {
    let mut h = start().await;
    h.frame().await;
    h.ctrl('b').await;
    assert!(h.frame().await.contains("new window"));

    h.key(KeyCode::Char('?')).await;
    assert!(
        h.frame().await.contains("\x1b[?25l"),
        "overlay hides the cursor"
    );
    h.key(KeyCode::Esc).await;
    assert!(h.mode_is_normal());
}
