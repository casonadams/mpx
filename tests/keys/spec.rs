use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::keys::KeySpec;

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

fn spec(text: &str) -> KeySpec {
    KeySpec::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn event(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

#[test]
fn parse_maps_names_prefixes_and_chars() {
    let cases: &[(&str, KeyCode, KeyModifiers)] = &[
        ("Enter", KeyCode::Enter, NONE),
        ("Esc", KeyCode::Esc, NONE),
        ("Escape", KeyCode::Esc, NONE),
        ("BSpace", KeyCode::Backspace, NONE),
        ("PgUp", KeyCode::PageUp, NONE),
        ("PPage", KeyCode::PageUp, NONE),
        ("NPage", KeyCode::PageDown, NONE),
        ("PgDn", KeyCode::PageDown, NONE),
        ("IC", KeyCode::Insert, NONE),
        ("DC", KeyCode::Delete, NONE),
        ("F1", KeyCode::F(1), NONE),
        ("F12", KeyCode::F(12), NONE),
        ("Space", KeyCode::Char(' '), NONE),
        ("h", KeyCode::Char('h'), NONE),
        ("H", KeyCode::Char('H'), NONE),
        ("C-b", KeyCode::Char('b'), CTRL),
        ("^b", KeyCode::Char('b'), CTRL),
        ("M-x", KeyCode::Char('x'), ALT),
        ("C-M-x", KeyCode::Char('x'), CTRL | ALT),
        ("S-Left", KeyCode::Left, SHIFT),
        ("BTab", KeyCode::BackTab, SHIFT),
        ("S-Tab", KeyCode::BackTab, SHIFT),
        ("C--", KeyCode::Char('-'), CTRL),
    ];
    for (text, code, mods) in cases {
        let key = spec(text);
        assert_eq!((key.code, key.mods), (*code, *mods), "{text}");
    }
}

#[test]
fn parse_rejects_empty_unknown_and_dangling_names() {
    for bad in ["", "Bogus", "F13", "F0", "C-", "C-Bogus", "Ctrl-x", "M-"] {
        assert!(KeySpec::parse(bad).is_err(), "{bad:?} should be rejected");
    }
}

#[test]
fn display_is_canonical_tmux_notation() {
    let cases: &[(&str, &str)] = &[
        ("C-b", "C-b"),
        ("^b", "C-b"),
        ("M-Left", "M-Left"),
        ("S-Tab", "BTab"),
        ("BTab", "BTab"),
        ("Space", "Space"),
        ("C-Space", "C-Space"),
        ("F5", "F5"),
        ("S-F5", "S-F5"),
        ("C-M-x", "C-M-x"),
        ("Insert", "IC"),
        ("Delete", "DC"),
        ("Esc", "Escape"),
        ("\"", "\""),
        ("H", "H"),
    ];
    for (text, shown) in cases {
        assert_eq!(spec(text).display(), *shown, "{text}");
    }
}

#[test]
fn parse_of_display_round_trips() {
    for text in [
        "C-b", "M-Left", "BTab", "C-M-x", "S-F5", "Space", "C--", "H", "Escape",
    ] {
        let key = spec(text);
        assert_eq!(spec(&key.display()), key, "{text}");
    }
}

#[test]
fn matches_compares_printable_chars_exactly_and_ignores_shift() {
    let h = spec("H");
    assert!(h.matches(&event(KeyCode::Char('H'), SHIFT)));
    assert!(h.matches(&event(KeyCode::Char('H'), NONE)));
    assert!(!h.matches(&event(KeyCode::Char('h'), NONE)));

    let lower = spec("h");
    assert!(!lower.matches(&event(KeyCode::Char('H'), NONE)));
}

#[test]
fn matches_ctrl_letters_regardless_of_reported_case() {
    let ctrl_b = spec("C-b");
    assert!(ctrl_b.matches(&event(KeyCode::Char('b'), CTRL)));
    assert!(ctrl_b.matches(&event(KeyCode::Char('B'), CTRL)));
    assert!(ctrl_b.matches(&event(KeyCode::Char('B'), CTRL | SHIFT)));
    assert!(!ctrl_b.matches(&event(KeyCode::Char('b'), NONE)));
    assert!(!ctrl_b.matches(&event(KeyCode::Char('b'), CTRL | ALT)));
}

#[test]
fn matches_backtab_with_or_without_reported_shift() {
    let btab = spec("S-Tab");
    assert!(btab.matches(&event(KeyCode::BackTab, SHIFT)));
    assert!(btab.matches(&event(KeyCode::BackTab, NONE)));
    assert!(btab.matches(&event(KeyCode::Tab, SHIFT)));
    assert!(!spec("Tab").matches(&event(KeyCode::BackTab, NONE)));
}

#[test]
fn matches_keeps_shift_for_non_printable_keys() {
    assert!(!spec("Left").matches(&event(KeyCode::Left, SHIFT)));
    assert!(spec("S-Left").matches(&event(KeyCode::Left, SHIFT)));
}
