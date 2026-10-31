//! Copy mode driven by command lines: `copy-mode` and `send-keys -X` run as typed at the prompt.
//! Also the copy-mode key tables a `bind-key -T copy-mode-vi` changes, and `#{pane_in_mode}`.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use mpx::config::ModeKeys;
use mpx::tui::app::Mode;

use super::support::{Harness, long_output, two_lines};

/// Runs `line` as typed at the `:` prompt, without the prompt itself.
async fn send(h: &mut Harness, line: &str) {
    match mpx::tui::command::parse(line) {
        Ok(cmd) => {
            h.app.run(cmd).await;
        }
        Err(e) => {
            h.app.say(e);
        }
    }
    // Any event ends in the app's post-input work: copies reach the paste buffers.
    h.app.on_event(Event::FocusGained).await;
}

/// Runs each of `lines` in order with `send`.
async fn send_all(h: &mut Harness, lines: &[&str]) {
    for line in lines {
        send(h, line).await;
    }
}

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Enters copy mode, then binds `command` to `key` in copy-mode-vi (an empty command unbinds it).
async fn set_copy_vi_key(h: &mut Harness, key: char, command: &str) {
    h.prefixed_char('[').await;
    h.probe
        .set_binding(&format!("copy-mode-vi:{key}"), Some(command))
        .await
        .unwrap();
    let bound = !command.is_empty();
    h.until(&format!("{key} bound: {bound}"), move |app| {
        app.keymap.copy_action(ModeKeys::Vi, &press(key)).is_some() == bound
    })
    .await;
}

fn scroll_offset(h: &Harness) -> Option<usize> {
    match &h.app.mode {
        Mode::Copy(cs) => Some(cs.scroll_offset),
        _ => None,
    }
}

#[tokio::test]
async fn send_keys_selects_and_copy_selection_and_cancel_fills_the_buffer_and_clipboard() {
    let mut h = Harness::started(&two_lines()).await;
    send(&mut h, "copy-mode").await;
    assert!(matches!(h.app.mode, Mode::Copy(_)));

    send(&mut h, "send-keys -X history-top").await;
    send(&mut h, "send-keys -X begin-selection").await;
    send(&mut h, "send-keys -X -N 4 cursor-right").await;
    send(&mut h, "send-keys -X copy-selection-and-cancel").await;

    assert_eq!(h.clipboard(), ["hello"]);
    assert!(h.mode_is_normal());
    assert_eq!(h.probe.get_buffer(None).await.unwrap().data, "hello");
}

#[tokio::test]
async fn rectangle_toggle_from_a_script_copies_the_block_between_the_corners() {
    let mut h = Harness::started(&two_lines()).await;
    send_all(
        &mut h,
        &[
            "copy-mode",
            "send-keys -X history-top",
            "send-keys -X rectangle-toggle",
            "send-keys -X begin-selection",
            "send-keys -X -N 4 cursor-right",
            "send-keys -X cursor-down",
            "send-keys -X copy-selection-and-cancel",
        ],
    )
    .await;
    assert_eq!(h.clipboard(), ["hello\nREADY"]);
}

#[tokio::test]
async fn copy_mode_u_starts_a_page_up_and_q_leaves_copy_mode() {
    let mut h = Harness::started(&long_output()).await;
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
}

/// A copy-mode key bound from a script runs its command; an unbound one stops working.
#[tokio::test]
async fn copy_vi_keys_follow_their_bindings_from_a_script() {
    // (key, command bound to it, whether the key leaves copy mode when pressed)
    let cases = [('x', "send-keys -X cancel", true), ('q', "", false)];
    for (key, command, leaves_copy_mode) in cases {
        let mut h = Harness::started(&two_lines()).await;
        set_copy_vi_key(&mut h, key, command).await;
        h.key(KeyCode::Char(key)).await;
        if leaves_copy_mode {
            assert!(h.mode_is_normal(), "{key} cancels copy mode");
        } else {
            assert!(
                matches!(h.app.mode, Mode::Copy(_)),
                "{key} no longer leaves copy mode"
            );
        }
    }
}

#[tokio::test]
async fn pane_in_mode_is_one_while_the_pane_is_in_copy_mode() {
    let mut h = Harness::started(&two_lines()).await;
    let in_mode = |h: &Harness| mpx::tui::frame::expand_active(&h.app, "#{pane_in_mode}");
    assert_eq!(in_mode(&h), "0");
    h.prefixed_char('[').await;
    assert_eq!(in_mode(&h), "1");
    h.key(KeyCode::Char('q')).await;
    assert_eq!(in_mode(&h), "0");
}

#[tokio::test]
async fn a_bad_copy_command_is_refused_and_leaves_copy_mode_as_it_was() {
    let mut h = Harness::started(&two_lines()).await;
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
