use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::keys::{KeySpec, TermModes, encode_key, encode_key_names, focus_event, wrap_paste};

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

fn plain() -> TermModes {
    TermModes::default()
}

fn app_cursor() -> TermModes {
    TermModes {
        app_cursor: true,
        ..TermModes::default()
    }
}

fn enc(code: KeyCode, mods: KeyModifiers, modes: &TermModes) -> Vec<u8> {
    encode_key(&KeyEvent::new(code, mods), modes)
}

fn names(args: &[&str], modes: &TermModes) -> Vec<u8> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    encode_key_names(&owned, modes)
}

#[test]
fn encode_key_table() {
    let cases: &[(KeyCode, KeyModifiers, &str)] = &[
        (KeyCode::Char('a'), NONE, "a"),
        (KeyCode::Char('é'), NONE, "é"),
        (KeyCode::Char('b'), CTRL, "\x02"),
        (KeyCode::Char('a'), CTRL, "\x01"),
        (KeyCode::Char('z'), CTRL, "\x1a"),
        (KeyCode::Char('B'), CTRL, "\x02"),
        (KeyCode::Char(' '), CTRL, "\x00"),
        (KeyCode::Char('@'), CTRL, "\x00"),
        (KeyCode::Char('['), CTRL, "\x1b"),
        (KeyCode::Char('\\'), CTRL, "\x1c"),
        (KeyCode::Char(']'), CTRL, "\x1d"),
        (KeyCode::Char('^'), CTRL, "\x1e"),
        (KeyCode::Char('_'), CTRL, "\x1f"),
        (KeyCode::Char('x'), ALT, "\x1bx"),
        (KeyCode::Char('b'), ALT | CTRL, "\x1b\x02"),
        (KeyCode::Enter, NONE, "\r"),
        (KeyCode::Enter, SHIFT, "\x1b\r"),
        (KeyCode::Enter, ALT, "\x1b\r"),
        (KeyCode::Tab, NONE, "\t"),
        (KeyCode::Tab, ALT, "\x1b\t"),
        (KeyCode::BackTab, SHIFT, "\x1b[Z"),
        (KeyCode::BackTab, NONE, "\x1b[Z"),
        (KeyCode::Backspace, NONE, "\x7f"),
        (KeyCode::Backspace, CTRL, "\x08"),
        (KeyCode::Backspace, ALT, "\x1b\x7f"),
        (KeyCode::Esc, NONE, "\x1b"),
        (KeyCode::Up, NONE, "\x1b[A"),
        (KeyCode::Down, NONE, "\x1b[B"),
        (KeyCode::Right, NONE, "\x1b[C"),
        (KeyCode::Left, NONE, "\x1b[D"),
        (KeyCode::Home, NONE, "\x1b[H"),
        (KeyCode::End, NONE, "\x1b[F"),
        (KeyCode::Left, CTRL, "\x1b[1;5D"),
        (KeyCode::Right, SHIFT, "\x1b[1;2C"),
        (KeyCode::Up, ALT, "\x1b[1;3A"),
        (KeyCode::Up, SHIFT | ALT | CTRL, "\x1b[1;8A"),
        (KeyCode::Insert, NONE, "\x1b[2~"),
        (KeyCode::Delete, NONE, "\x1b[3~"),
        (KeyCode::PageUp, NONE, "\x1b[5~"),
        (KeyCode::PageDown, NONE, "\x1b[6~"),
        (KeyCode::Delete, CTRL, "\x1b[3;5~"),
        (KeyCode::PageUp, SHIFT, "\x1b[5;2~"),
        (KeyCode::F(1), NONE, "\x1bOP"),
        (KeyCode::F(4), NONE, "\x1bOS"),
        (KeyCode::F(1), CTRL, "\x1b[1;5P"),
        (KeyCode::F(5), NONE, "\x1b[15~"),
        (KeyCode::F(6), NONE, "\x1b[17~"),
        (KeyCode::F(12), NONE, "\x1b[24~"),
        (KeyCode::F(5), SHIFT, "\x1b[15;2~"),
        (KeyCode::F(13), NONE, ""),
        (KeyCode::Null, NONE, ""),
    ];
    for (code, mods, expected) in cases {
        assert_eq!(
            enc(*code, *mods, &plain()),
            expected.as_bytes(),
            "{code:?} {mods:?}"
        );
    }
}

#[test]
fn decckm_switches_unmodified_cursor_keys_to_ss3() {
    let cases: &[(KeyCode, &str)] = &[
        (KeyCode::Up, "\x1bOA"),
        (KeyCode::Down, "\x1bOB"),
        (KeyCode::Right, "\x1bOC"),
        (KeyCode::Left, "\x1bOD"),
        (KeyCode::Home, "\x1bOH"),
        (KeyCode::End, "\x1bOF"),
    ];
    for (code, expected) in cases {
        assert_eq!(
            enc(*code, NONE, &app_cursor()),
            expected.as_bytes(),
            "{code:?}"
        );
    }
}

#[test]
fn decckm_leaves_modified_and_function_keys_alone() {
    assert_eq!(enc(KeyCode::Left, CTRL, &app_cursor()), b"\x1b[1;5D");
    assert_eq!(enc(KeyCode::F(1), NONE, &app_cursor()), b"\x1bOP");
    assert_eq!(enc(KeyCode::PageUp, NONE, &app_cursor()), b"\x1b[5~");
}

#[test]
fn key_names_encode_when_they_parse_and_literal_text_otherwise() {
    assert_eq!(names(&["C-c"], &plain()), b"\x03");
    assert_eq!(names(&["S-Tab"], &plain()), b"\x1b[Z");
    assert_eq!(names(&["M-x"], &plain()), b"\x1bx");
    assert_eq!(names(&["Enter"], &plain()), b"\r");
    assert_eq!(names(&["Escape"], &plain()), b"\x1b");
    assert_eq!(names(&["Space"], &plain()), b" ");
    assert_eq!(names(&["^c"], &plain()), b"\x03");
    assert_eq!(names(&["hello"], &plain()), b"hello");
    assert_eq!(names(&["C-"], &plain()), b"C-");
    assert_eq!(names(&["x"], &plain()), b"x");
    assert_eq!(names(&["H"], &plain()), b"H");
    assert_eq!(names(&["é"], &plain()), "é".as_bytes());
    assert!(names(&[], &plain()).is_empty());
}

#[test]
fn key_names_concatenate_in_order() {
    assert_eq!(names(&["l", "s", "Enter"], &plain()), b"ls\r");
    assert_eq!(names(&["C-u", "ls", "Enter"], &plain()), b"\x15ls\r");
}

#[test]
fn key_names_honour_decckm_for_arrows() {
    assert_eq!(names(&["Up"], &app_cursor()), b"\x1bOA");
    assert_eq!(names(&["Up"], &plain()), b"\x1b[A");
}

#[test]
fn key_spec_encode_matches_encode_key() {
    let up = KeySpec::parse("S-Up").unwrap();
    assert_eq!(up.encode(&plain()), enc(KeyCode::Up, SHIFT, &plain()));
    let ctrl_c = KeySpec::parse("C-c").unwrap();
    assert_eq!(ctrl_c.encode(&plain()), b"\x03");
}

#[test]
fn wrap_paste_plain_normalizes_newlines_without_markers() {
    let modes = plain();
    assert_eq!(wrap_paste("a\nb", &modes), b"a\rb");
    assert_eq!(wrap_paste("a\r\nb", &modes), b"a\rb");
    assert_eq!(wrap_paste("a\r\n\r\nb", &modes), b"a\r\rb");
    assert_eq!(wrap_paste("x\x1b[201~y", &modes), b"x\x1b[201~y");
}

#[test]
fn wrap_paste_bracketed_adds_markers() {
    let modes = TermModes {
        bracketed_paste: true,
        ..TermModes::default()
    };
    assert_eq!(wrap_paste("hi", &modes), b"\x1b[200~hi\x1b[201~");
    assert_eq!(wrap_paste("a\nb", &modes), b"\x1b[200~a\rb\x1b[201~");
    assert_eq!(wrap_paste("", &modes), b"\x1b[200~\x1b[201~");
}

#[test]
fn wrap_paste_bracketed_strips_embedded_end_markers() {
    let modes = TermModes {
        bracketed_paste: true,
        ..TermModes::default()
    };
    assert_eq!(wrap_paste("x\x1b[201~y", &modes), b"\x1b[200~xy\x1b[201~");
    assert_eq!(
        wrap_paste("\x1b[20\x1b[201~1~", &modes),
        b"\x1b[200~\x1b[201~"
    );
}

#[test]
fn focus_event_reports_focus_changes() {
    assert_eq!(focus_event(true), b"\x1b[I");
    assert_eq!(focus_event(false), b"\x1b[O");
}
