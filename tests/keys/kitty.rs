use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use mpx::config::ExtendedKeys;
use mpx::keys::{KeyPolicy, TermModes, encode_key};

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

const DISAMBIGUATE: u8 = 0b1;
const EVENTS: u8 = 0b10;
const ALTERNATES: u8 = 0b100;
const ALL_KEYS: u8 = 0b1000;
const TEXT: u8 = 0b1_0000;

fn policy() -> KeyPolicy {
    KeyPolicy {
        extended: ExtendedKeys::On,
        xterm_keys: true,
    }
}

fn modes(flags: u8) -> TermModes {
    TermModes {
        kitty_keys: flags,
        ..TermModes::default()
    }
}

fn press(code: KeyCode, mods: KeyModifiers, flags: u8) -> Vec<u8> {
    encode_key(&KeyEvent::new(code, mods), &modes(flags), policy())
}

fn event(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind, flags: u8) -> Vec<u8> {
    let ev = KeyEvent::new_with_kind(code, mods, kind);
    encode_key(&ev, &modes(flags), policy())
}

#[test]
fn report_all_keys_sends_text_keys_as_csi_u() {
    let f = ALL_KEYS;
    assert_eq!(press(KeyCode::Char('a'), NONE, f), b"\x1b[97u");
    assert_eq!(press(KeyCode::Char('A'), SHIFT, f), b"\x1b[97;2u");
    assert_eq!(press(KeyCode::Char(' '), NONE, f), b"\x1b[32u");
    assert_eq!(press(KeyCode::Enter, NONE, f), b"\x1b[13u");
    assert_eq!(press(KeyCode::Tab, NONE, f), b"\x1b[9u");
    assert_eq!(press(KeyCode::Backspace, NONE, f), b"\x1b[127u");
    assert_eq!(press(KeyCode::Esc, NONE, f), b"\x1b[27u");
    assert_eq!(press(KeyCode::Char('a'), CTRL, f), b"\x1b[97;5u");
    assert_eq!(press(KeyCode::BackTab, SHIFT, f), b"\x1b[9;2u");
}

#[test]
fn report_all_keys_keeps_functional_keys_in_legacy_form() {
    let f = ALL_KEYS;
    assert_eq!(press(KeyCode::Left, NONE, f), b"\x1b[D");
    assert_eq!(press(KeyCode::Up, CTRL, f), b"\x1b[1;5A");
    assert_eq!(press(KeyCode::F(5), NONE, f), b"\x1b[15~");
    assert_eq!(press(KeyCode::Delete, SHIFT, f), b"\x1b[3;2~");
}

#[test]
fn alternate_keys_add_the_shifted_code_to_shifted_letters() {
    assert_eq!(press(KeyCode::Char('A'), SHIFT, ALL_KEYS | ALTERNATES), b"\x1b[97:65;2u");
    assert_eq!(press(KeyCode::Char('a'), NONE, ALL_KEYS | ALTERNATES), b"\x1b[97u");
    assert_eq!(press(KeyCode::Char('A'), SHIFT, ALL_KEYS), b"\x1b[97;2u");
    assert_eq!(press(KeyCode::Char('x'), CTRL, ALTERNATES | DISAMBIGUATE), b"\x1b[120;5u");
}

#[test]
fn associated_text_follows_report_all_keys_and_is_the_text_codepoint() {
    let f = ALL_KEYS | TEXT;
    assert_eq!(press(KeyCode::Char('a'), NONE, f), b"\x1b[97;;97u");
    assert_eq!(press(KeyCode::Char('A'), SHIFT, f), b"\x1b[97;2;65u");
    assert_eq!(press(KeyCode::Char('a'), CTRL, f), b"\x1b[97;5u");
    assert_eq!(press(KeyCode::Char('A'), SHIFT, ALL_KEYS | ALTERNATES | TEXT), b"\x1b[97:65;2;65u");
    assert_eq!(press(KeyCode::Char('a'), NONE, TEXT), b"a");
}

#[test]
fn event_types_mark_repeat_and_release_on_csi_u_keys() {
    let f = EVENTS | ALL_KEYS;
    assert_eq!(event(KeyCode::Char('a'), NONE, KeyEventKind::Repeat, f), b"\x1b[97;1:2u");
    assert_eq!(event(KeyCode::Char('a'), NONE, KeyEventKind::Release, f), b"\x1b[97;1:3u");
    assert_eq!(event(KeyCode::Enter, NONE, KeyEventKind::Release, f), b"\x1b[13;1:3u");
    assert_eq!(event(KeyCode::Char('a'), NONE, KeyEventKind::Press, f), b"\x1b[97u");
}

#[test]
fn event_types_mark_functional_keys_with_the_event_field() {
    let f = EVENTS;
    assert_eq!(event(KeyCode::Left, NONE, KeyEventKind::Press, f), b"\x1b[D");
    assert_eq!(event(KeyCode::Left, NONE, KeyEventKind::Repeat, f), b"\x1b[1;1:2D");
    assert_eq!(event(KeyCode::Left, NONE, KeyEventKind::Release, f), b"\x1b[1;1:3D");
    assert_eq!(event(KeyCode::Left, CTRL, KeyEventKind::Release, f), b"\x1b[1;5:3D");
    assert_eq!(event(KeyCode::F(1), NONE, KeyEventKind::Release, f), b"\x1b[1;1:3P");
    assert_eq!(event(KeyCode::Delete, NONE, KeyEventKind::Release, f), b"\x1b[3;1:3~");
}

#[test]
fn event_types_drop_releases_of_text_keys_unless_all_keys_are_reported() {
    assert!(event(KeyCode::Char('a'), NONE, KeyEventKind::Release, EVENTS).is_empty());
    assert!(event(KeyCode::Enter, NONE, KeyEventKind::Release, EVENTS).is_empty());
    assert!(event(KeyCode::Backspace, NONE, KeyEventKind::Release, EVENTS).is_empty());
    assert_eq!(event(KeyCode::Char('a'), NONE, KeyEventKind::Repeat, EVENTS), b"a");
    assert_eq!(event(KeyCode::Enter, NONE, KeyEventKind::Repeat, EVENTS), b"\r");
}

#[test]
fn event_types_report_escape_and_ctrl_keys_as_csi_u() {
    assert_eq!(event(KeyCode::Esc, NONE, KeyEventKind::Release, EVENTS), b"\x1b[27;1:3u");
    assert_eq!(event(KeyCode::Esc, NONE, KeyEventKind::Press, EVENTS), b"\x1b");
    assert_eq!(event(KeyCode::Char('a'), CTRL, KeyEventKind::Repeat, EVENTS), b"\x1b[97;5:2u");
    assert_eq!(event(KeyCode::Char('a'), CTRL, KeyEventKind::Press, EVENTS), b"\x01");
}

#[test]
fn key_events_without_the_event_flag_ignore_their_kind() {
    assert_eq!(event(KeyCode::Left, NONE, KeyEventKind::Release, ALTERNATES), b"\x1b[D");
    assert_eq!(event(KeyCode::Left, NONE, KeyEventKind::Repeat, DISAMBIGUATE), b"\x1b[D");
}

#[test]
fn flags_without_an_encoding_change_leave_keys_alone() {
    assert_eq!(press(KeyCode::Char('a'), NONE, ALTERNATES | TEXT), b"a");
    assert_eq!(press(KeyCode::Char('a'), CTRL, ALTERNATES), b"\x01");
    assert_eq!(press(KeyCode::Enter, NONE, 0), b"\r");
}
