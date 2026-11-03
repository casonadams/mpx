//! display-popup and display-menu: a popup's pty lifecycle, `-E`, key routing, and a menu's
//! selection and the commands it runs.
use crossterm::event::KeyCode;

use mpx::config::Config;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, screen_text};

/// A harness on its first frame: where every popup and menu test starts.
async fn ready() -> Harness {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.frame().await;
    h
}

/// Runs `line`, which must open a menu.
async fn open_menu(h: &mut Harness, line: &str) {
    h.command(line).await;
    assert!(matches!(h.app.mode, Mode::Menu(_)), "menu opened");
}

#[tokio::test]
async fn a_popup_with_e_closes_when_its_command_exits() {
    let mut h = ready().await;
    h.command("display-popup -E -w 40 -h 10 'printf POPUP-OUT'")
        .await;
    assert!(matches!(h.app.mode, Mode::Popup(_)), "popup opened");
    h.until_popup("command exited", |popup| popup.exited).await;
    assert!(h.mode_is_normal(), "-E closes the popup at exit");
    assert_eq!(
        h.app.active_pane, MAIN,
        "the panes are back under the popup"
    );
}

#[tokio::test]
async fn a_popup_without_e_stays_up_after_its_command_and_the_next_key_closes_it() {
    let mut h = ready().await;
    h.command("display-popup -w 40 -h 10 'printf POPUP-OUT'")
        .await;
    h.until_popup("command exited", |popup| popup.exited).await;
    let drawn = h.frame().await;
    h.shown_then_closed(&drawn, &["POPUP-OUT"], KeyCode::Char('x'))
        .await;
}

#[tokio::test]
async fn keys_go_to_a_running_popup_and_not_to_the_panes() {
    let script = "stty raw -echo; printf READY; exec cat";
    let mut h = ready().await;
    h.command(&format!("display-popup -w 40 -h 10 '{script}'"))
        .await;
    h.until_popup("popup ready", |popup| screen_text(popup).contains("READY"))
        .await;
    h.type_str("hi").await;
    h.key(KeyCode::Enter).await;
    h.until_popup("popup echoes the keys", |popup| {
        screen_text(popup).contains("hi")
    })
    .await;
    assert!(
        matches!(h.app.mode, Mode::Popup(_)),
        "the popup stays open for typing"
    );
    let pane = h.logs(MAIN).await;
    assert!(
        !pane.contains("hi\r"),
        "keys typed into the popup never reach the pane"
    );
    h.ctrl('c').await;
}

#[tokio::test]
async fn a_menu_runs_its_selected_item_on_enter() {
    let mut h = ready().await;
    open_menu(&mut h, "display-menu -T Pick Zoom z 'resize-pane -Z'").await;
    let drawn = h.frame().await;
    assert!(drawn.contains("Zoom"), "the menu lists its items\n{drawn}");
    h.key(KeyCode::Enter).await;
    assert!(h.mode_is_normal(), "running an item closes the menu");
    h.frame().await;
    assert!(h.app.zoomed(), "the selected item zoomed the pane");
}

#[tokio::test]
async fn an_item_key_runs_it_and_a_disabled_item_never_does() {
    let mut h = ready().await;
    open_menu(
        &mut h,
        "display-menu Zoom z 'resize-pane -Z' -Off o 'resize-pane -Z'",
    )
    .await;
    h.key(KeyCode::Char('o')).await;
    assert!(
        matches!(h.app.mode, Mode::Menu(_)),
        "a disabled item keeps the menu open"
    );
    h.key(KeyCode::Char('z')).await;
    assert!(
        h.mode_is_normal(),
        "the item's key runs it and closes the menu"
    );
    h.frame().await;
    assert!(h.app.zoomed(), "the item zoomed the pane");
}

#[tokio::test]
async fn escape_closes_a_menu_without_running_anything() {
    let mut h = ready().await;
    h.command("display-menu Zoom z 'resize-pane -Z'").await;
    h.closes_with(|mode| matches!(mode, Mode::Menu(_)), KeyCode::Esc)
        .await;
    h.frame().await;
    assert!(!h.app.zoomed(), "nothing ran");
}
