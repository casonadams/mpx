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

fn event(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind, flags: u8) -> Vec<u8> {
    let ev = KeyEvent::new_with_kind(code, mods, kind);
    encode_key(&ev, &modes(flags), policy())
}

type Case = (KeyCode, KeyModifiers, KeyEventKind, u8, &'static [u8]);

/// Asserts each `(code, mods, kind, flags, expected)` encodes to its bytes; an empty expectation is
/// a release that the flags do not report.
fn assert_encodes(cases: &[Case]) {
    for (code, mods, kind, flags, want) in cases {
        assert_eq!(
            event(*code, *mods, *kind, *flags),
            *want,
            "{code:?} {mods:?} {kind:?} flags {flags:#07b}"
        );
    }
}

#[test]
fn kitty_protocol_encodes_text_and_functional_keys_by_the_flags() {
    assert_encodes(&[
        // report-all-keys: text keys as CSI u, functional keys keep their legacy form
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[97u",
        ),
        (
            KeyCode::Char('A'),
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[97;2u",
        ),
        (
            KeyCode::Char(' '),
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[32u",
        ),
        (
            KeyCode::Enter,
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[13u",
        ),
        (
            KeyCode::Tab,
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[9u",
        ),
        (
            KeyCode::Backspace,
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[127u",
        ),
        (
            KeyCode::Esc,
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[27u",
        ),
        (
            KeyCode::Char('a'),
            CTRL,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[97;5u",
        ),
        (
            KeyCode::BackTab,
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[9;2u",
        ),
        (
            KeyCode::Left,
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[D",
        ),
        (
            KeyCode::Up,
            CTRL,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[1;5A",
        ),
        (
            KeyCode::F(5),
            NONE,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[15~",
        ),
        (
            KeyCode::Delete,
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[3;2~",
        ),
        // alternate keys: the shifted code point follows the base code
        (
            KeyCode::Char('A'),
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS | ALTERNATES,
            b"\x1b[97:65;2u",
        ),
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Press,
            ALL_KEYS | ALTERNATES,
            b"\x1b[97u",
        ),
        (
            KeyCode::Char('A'),
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS,
            b"\x1b[97;2u",
        ),
        (
            KeyCode::Char('x'),
            CTRL,
            KeyEventKind::Press,
            ALTERNATES | DISAMBIGUATE,
            b"\x1b[120;5u",
        ),
        // associated text: the text code point follows report-all-keys
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Press,
            ALL_KEYS | TEXT,
            b"\x1b[97;;97u",
        ),
        (
            KeyCode::Char('A'),
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS | TEXT,
            b"\x1b[97;2;65u",
        ),
        (
            KeyCode::Char('a'),
            CTRL,
            KeyEventKind::Press,
            ALL_KEYS | TEXT,
            b"\x1b[97;5u",
        ),
        (
            KeyCode::Char('A'),
            SHIFT,
            KeyEventKind::Press,
            ALL_KEYS | ALTERNATES | TEXT,
            b"\x1b[97:65;2;65u",
        ),
        (KeyCode::Char('a'), NONE, KeyEventKind::Press, TEXT, b"a"),
    ]);
}

#[test]
fn kitty_protocol_encodes_event_types_and_drops_flagless_releases() {
    assert_encodes(&[
        // event types on CSI u keys
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Repeat,
            EVENTS | ALL_KEYS,
            b"\x1b[97;1:2u",
        ),
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Release,
            EVENTS | ALL_KEYS,
            b"\x1b[97;1:3u",
        ),
        (
            KeyCode::Enter,
            NONE,
            KeyEventKind::Release,
            EVENTS | ALL_KEYS,
            b"\x1b[13;1:3u",
        ),
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Press,
            EVENTS | ALL_KEYS,
            b"\x1b[97u",
        ),
        // event types on functional keys carry the event field
        (KeyCode::Left, NONE, KeyEventKind::Press, EVENTS, b"\x1b[D"),
        (
            KeyCode::Left,
            NONE,
            KeyEventKind::Repeat,
            EVENTS,
            b"\x1b[1;1:2D",
        ),
        (
            KeyCode::Left,
            NONE,
            KeyEventKind::Release,
            EVENTS,
            b"\x1b[1;1:3D",
        ),
        (
            KeyCode::Left,
            CTRL,
            KeyEventKind::Release,
            EVENTS,
            b"\x1b[1;5:3D",
        ),
        (
            KeyCode::F(1),
            NONE,
            KeyEventKind::Release,
            EVENTS,
            b"\x1b[1;1:3P",
        ),
        (
            KeyCode::Delete,
            NONE,
            KeyEventKind::Release,
            EVENTS,
            b"\x1b[3;1:3~",
        ),
        // releases of text keys are dropped unless all keys are reported
        (KeyCode::Char('a'), NONE, KeyEventKind::Release, EVENTS, b""),
        (KeyCode::Enter, NONE, KeyEventKind::Release, EVENTS, b""),
        (KeyCode::Backspace, NONE, KeyEventKind::Release, EVENTS, b""),
        (KeyCode::Char('a'), NONE, KeyEventKind::Repeat, EVENTS, b"a"),
        (KeyCode::Enter, NONE, KeyEventKind::Repeat, EVENTS, b"\r"),
        // escape and ctrl keys are CSI u once events are reported
        (
            KeyCode::Esc,
            NONE,
            KeyEventKind::Release,
            EVENTS,
            b"\x1b[27;1:3u",
        ),
        (KeyCode::Esc, NONE, KeyEventKind::Press, EVENTS, b"\x1b"),
        (
            KeyCode::Char('a'),
            CTRL,
            KeyEventKind::Repeat,
            EVENTS,
            b"\x1b[97;5:2u",
        ),
        (
            KeyCode::Char('a'),
            CTRL,
            KeyEventKind::Press,
            EVENTS,
            b"\x01",
        ),
        // without the event flag the kind is ignored
        (
            KeyCode::Left,
            NONE,
            KeyEventKind::Release,
            ALTERNATES,
            b"\x1b[D",
        ),
        (
            KeyCode::Left,
            NONE,
            KeyEventKind::Repeat,
            DISAMBIGUATE,
            b"\x1b[D",
        ),
        // flags that change no encoding leave keys alone
        (
            KeyCode::Char('a'),
            NONE,
            KeyEventKind::Press,
            ALTERNATES | TEXT,
            b"a",
        ),
        (
            KeyCode::Char('a'),
            CTRL,
            KeyEventKind::Press,
            ALTERNATES,
            b"\x01",
        ),
        (KeyCode::Enter, NONE, KeyEventKind::Press, 0, b"\r"),
    ]);
}
