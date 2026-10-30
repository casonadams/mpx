//! Copy mode driven the way scripts drive it: `copy-mode` and `send-keys -X` are sent to the
//! attached terminal, which applies them and acknowledges them. Also the copy-mode key tables a
//! `bind-key -T copy-mode-vi` changes, and the pane modes the terminal reports.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::config::{Config, ModeKeys};
use mpx::protocol::VarKind;
use mpx::tui::app::Mode;

use super::support::{ECHO, Harness, MAIN, SESSION, connect};

/// Prints `hello world`, then `READY` on the second row.
fn two_lines() -> String {
    format!(r"printf 'hello world\r\n'; {ECHO}")
}

/// Prints 40 numbered lines, so the pane has scrollback.
fn long_output() -> String {
    format!(r"for i in $(seq 1 40); do printf 'line %s\r\n' $i; done; {ECHO}")
}

async fn started(script: &str) -> Harness {
    let mut h = Harness::start(Config::default(), script).await;
    h.frame().await;
    h.until("pane text drawn", |app| {
        app.active_vt()
            .is_some_and(|vt| vt.snapshot().contains("READY"))
    })
    .await;
    h
}

/// Sends `line` as the shim sends a command, then runs the terminal until it has applied it.
async fn send(h: &mut Harness, line: &str) {
    let endpoint = h.sandbox.endpoint(SESSION);
    let mut sender = connect(&endpoint).await;
    let seq = sender.client_command(line).await.expect("client command");
    let mut waiter = connect(&endpoint).await;
    let mut settled = tokio::spawn(async move { waiter.wait_command(seq).await });
    loop {
        let item = tokio::select! {
            done = &mut settled => {
                done.expect("waiter").expect("command settled");
                break;
            }
            item = h.rx.recv() => item,
        };
        let item = item.expect("event stream closed");
        h.app.on_stream(Some(item), &mut h.rx).await;
    }
}

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn scroll_offset(h: &Harness) -> Option<usize> {
    match &h.app.mode {
        Mode::Copy(cs) => Some(cs.scroll_offset),
        _ => None,
    }
}

#[tokio::test]
async fn send_keys_selects_and_copy_selection_and_cancel_fills_the_buffer_and_clipboard() {
    let mut h = started(&two_lines()).await;
    send(&mut h, "copy-mode").await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));
    assert_eq!(h.probe.pane_modes().await.unwrap(), [MAIN]);

    send(&mut h, "send-keys -X history-top").await;
    send(&mut h, "send-keys -X begin-selection").await;
    send(&mut h, "send-keys -X -N 4 cursor-right").await;
    send(&mut h, "send-keys -X copy-selection-and-cancel").await;

    assert_eq!(h.clipboard(), ["hello"]);
    assert!(h.mode_is_normal());
    assert!(h.probe.pane_modes().await.unwrap().is_empty());
    assert_eq!(h.probe.get_buffer(None).await.unwrap().data, "hello");
}

#[tokio::test]
async fn rectangle_toggle_from_a_script_copies_the_block_between_the_corners() {
    let mut h = started(&two_lines()).await;
    send(&mut h, "copy-mode").await;
    send(&mut h, "send-keys -X history-top").await;
    send(&mut h, "send-keys -X rectangle-toggle").await;
    send(&mut h, "send-keys -X begin-selection").await;
    send(&mut h, "send-keys -X -N 4 cursor-right").await;
    send(&mut h, "send-keys -X cursor-down").await;
    send(&mut h, "send-keys -X copy-selection-and-cancel").await;
    assert_eq!(h.clipboard(), ["hello\nREADY"]);
}

#[tokio::test]
async fn copy_mode_u_starts_a_page_up_and_q_leaves_copy_mode() {
    let mut h = started(&long_output()).await;
    send(&mut h, "copy-mode").await;
    assert_eq!(
        scroll_offset(&h),
        Some(0),
        "plain copy-mode starts at the live screen"
    );
    send(&mut h, "copy-mode -q").await;
    assert!(h.mode_is_normal());

    send(&mut h, "copy-mode -u").await;
    assert!(
        scroll_offset(&h).is_some_and(|offset| offset > 0),
        "-u pages up into history"
    );
    send(&mut h, "copy-mode -q").await;
    assert!(h.mode_is_normal());
    assert!(h.probe.pane_modes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_copy_mode_key_bound_from_a_script_runs_in_copy_mode() {
    let mut h = started(&two_lines()).await;
    h.prefixed_char('[').await;
    h.probe
        .set_var(
            VarKind::Binding,
            "copy-mode-vi:x",
            Some("send-keys -X cancel"),
        )
        .await
        .unwrap();
    h.until("x is bound in copy-mode-vi", |app| {
        app.keymap.copy_action(ModeKeys::Vi, &press('x')).is_some()
    })
    .await;
    h.key(KeyCode::Char('x')).await;
    assert!(h.mode_is_normal(), "x cancels copy mode");
}

#[tokio::test]
async fn unbinding_a_copy_mode_key_from_a_script_stops_it_working_in_copy_mode() {
    let mut h = started(&two_lines()).await;
    h.prefixed_char('[').await;
    h.probe
        .set_var(VarKind::Binding, "copy-mode-vi:q", Some(""))
        .await
        .unwrap();
    h.until("q is unbound in copy-mode-vi", |app| {
        app.keymap.copy_action(ModeKeys::Vi, &press('q')).is_none()
    })
    .await;
    h.key(KeyCode::Char('q')).await;
    assert!(
        matches!(h.app.mode, Mode::Copy(_)),
        "q no longer leaves copy mode"
    );
}

#[tokio::test]
async fn the_keyboard_copy_mode_is_reported_as_pane_in_mode() {
    let mut h = started(&two_lines()).await;
    assert!(h.probe.pane_modes().await.unwrap().is_empty());
    h.prefixed_char('[').await;
    assert_eq!(h.probe.pane_modes().await.unwrap(), [MAIN]);
    h.key(KeyCode::Char('q')).await;
    assert!(h.probe.pane_modes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_bad_copy_command_is_refused_and_leaves_copy_mode_as_it_was() {
    let mut h = started(&two_lines()).await;
    send(&mut h, "copy-mode").await;
    send(&mut h, "send-keys -X no-such-command").await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));
    assert!(
        h.app
            .message
            .as_ref()
            .is_some_and(|(m, _)| m.contains("copy-mode command not supported")),
        "{:?}",
        h.app.message
    );
}
