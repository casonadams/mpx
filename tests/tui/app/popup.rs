use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

use mpx::config::Config;
use mpx::protocol::{PopupExit, PopupOpts, PopupSpec, SpawnOpts};
use mpx::tui::app::{App, Mode};
use mpx::tui::popup::{area, inner_rect};

use super::fidelity::check_masked;
use super::support::{ECHO, Harness, MAIN, SIZE};

/// Opens a popup running `sh -c script` the way a script does (through the daemon), and waits until
/// the app shows it. Returns the popup's pane name.
pub(super) async fn open(h: &mut Harness, script: &str, exit: PopupExit) -> String {
    let opts = PopupOpts {
        spawn: SpawnOpts {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            pty: true,
            ..SpawnOpts::default()
        },
        spec: PopupSpec::default(),
        exit,
        wait: false,
    };
    h.probe.popup(opts).await.expect("open popup");
    h.until("popup shown", |app| matches!(app.mode, Mode::Popup(_)))
        .await;
    popup_name(&h.app)
}

/// The pane name of the popup on screen.
pub(super) fn popup_name(app: &App) -> String {
    match &app.mode {
        Mode::Popup(view) => view.name.clone(),
        _ => panic!("no popup on screen"),
    }
}

/// What the popup's program shows.
pub(super) fn popup_screen(app: &App) -> String {
    let name = popup_name(app);
    app.vt
        .get(&name)
        .map(|vt| vt.snapshot())
        .unwrap_or_default()
}

/// Top row of the pane area (below the status row when it is on top).
fn pane_top(h: &Harness) -> u16 {
    h.app.rows.pane_top
}

#[tokio::test]
async fn a_popup_is_drawn_centred_with_a_border_and_shows_its_program() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    open(&mut h, "printf hello; exec cat", PopupExit::Stay).await;
    h.until("popup shows its output", |app| {
        popup_screen(app).contains("hello")
    })
    .await;
    let shown = h.frame().await;

    let rect = area(&h.app).expect("popup area");
    let pane_h = h.app.viewport.1;
    assert_eq!(
        rect.height,
        pane_h * 50 / 100,
        "half the pane area's height"
    );
    assert_eq!(rect.width, SIZE.0 * 50 / 100, "half the width");
    assert_eq!(rect.x, (SIZE.0 - rect.width) / 2, "centred across");
    assert_eq!(
        rect.y,
        pane_top(&h) + (pane_h - rect.height) / 2,
        "centred down"
    );

    let cell = |x: u16, y: u16| {
        h.terminal
            .grid()
            .get_cell(x as usize, y as usize)
            .map(|c| c.c)
    };
    assert_eq!(cell(rect.x, rect.y), Some('┌'), "top-left corner");
    assert_eq!(
        cell(rect.x + rect.width - 1, rect.y + rect.height - 1),
        Some('┘'),
        "bottom-right corner"
    );
    assert!(shown.contains("hello"), "{shown}");
    // The terminal cursor is the program's, after its output, inside the popup's border.
    let cursor = h.cursor();
    assert_eq!(
        (cursor.x, cursor.y),
        (usize::from(rect.x) + 1 + "hello".len(), usize::from(rect.y) + 1),
        "cursor after the program's output"
    );
}

#[tokio::test]
async fn keys_go_to_the_popup_and_not_to_the_pane_beneath() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    open(&mut h, ECHO, PopupExit::Stay).await;
    h.until("popup ready", |app| popup_screen(app).contains("READY"))
        .await;

    h.type_str("xyz").await;
    h.until("popup echoes the keys", |app| {
        popup_screen(app).contains("xyz")
    })
    .await;
    assert!(
        !h.logs(MAIN).await.contains("xyz"),
        "the pane beneath got none of the keys"
    );

    h.mouse(MouseEventKind::Down(MouseButton::Left), 2, 2).await;
    assert!(
        matches!(h.app.mode, Mode::Popup(_)),
        "mouse input does not dismiss or move the popup"
    );
}

#[tokio::test]
async fn a_popup_closes_when_its_program_exits_under_e() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    open(&mut h, "printf done", PopupExit::Close).await;
    h.until("popup closed", |app| matches!(app.mode, Mode::Normal))
        .await;
    assert!(area(&h.app).is_none(), "nothing is drawn over the panes");
    check_masked(&mut h, "popup closed", &[]).await;
}

#[tokio::test]
async fn a_finished_popup_stays_until_a_key_closes_it_and_that_key_is_not_forwarded() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    open(&mut h, "printf bye", PopupExit::Stay).await;
    h.until(
        "popup finished",
        |app| matches!(&app.mode, Mode::Popup(view) if view.finished),
    )
    .await;
    let shown = h.frame().await;
    assert!(shown.contains("press a key to close"), "{shown}");
    assert!(popup_screen(&h.app).contains("bye"), "its output stays");

    h.key(KeyCode::Char('q')).await;
    assert!(h.mode_is_normal(), "a key closes a finished popup");
    assert!(
        !h.logs(MAIN).await.contains('q'),
        "the key that closed the popup went nowhere else"
    );
}

#[tokio::test]
async fn closing_the_popup_stops_a_running_program() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    open(&mut h, "exec sleep 30", PopupExit::Stay).await;
    h.probe.close_popup().await.expect("close");
    h.until("popup closed", |app| matches!(app.mode, Mode::Normal))
        .await;
}

#[tokio::test]
async fn a_resized_terminal_moves_the_popup_and_resizes_its_program() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    let name = open(&mut h, "exec cat", PopupExit::Stay).await;
    h.frame().await;
    let before = area(&h.app).expect("popup area");

    h.app.fixed_size = Some((60, 20));
    h.frame().await;
    let after = area(&h.app).expect("popup area");
    assert_eq!(after.width, 30, "half of the new width");
    assert!(after.width < before.width);

    let inner = inner_rect(after, true);
    let node = h.probe.status(&name).await.expect("status");
    assert_eq!(
        node.size,
        Some((inner.width, inner.height)),
        "the program is sized to the popup's inner area"
    );
}

#[tokio::test]
async fn the_prompt_opens_a_popup_at_the_given_place() {
    let mut h = Harness::start(Config::default(), ECHO).await;
    h.prefixed_char(':').await;
    h.type_str("display-popup -w 40 -h 10 -x 2 -y 3 cat").await;
    h.key(KeyCode::Enter).await;
    h.until("popup shown", |app| matches!(app.mode, Mode::Popup(_)))
        .await;

    let rect = area(&h.app).expect("popup area");
    assert_eq!(rect.x, 2);
    assert_eq!(rect.y, pane_top(&h) + 3);
    assert_eq!((rect.width, rect.height), (40, 10));
}
