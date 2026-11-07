use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

use super::csi::param_field;

/// Kitty keyboard protocol flags, as the program sets them.
const KITTY_DISAMBIGUATE: u8 = 0b1;
const KITTY_EVENTS: u8 = 0b10;
const KITTY_ALTERNATES: u8 = 0b100;
const KITTY_ALL_KEYS: u8 = 0b1000;
const KITTY_TEXT: u8 = 0b1_0000;

/// The kitty event type field of a key event under `flags`: press is implied (0), repeat 2,
/// release 3. Zero when the flags do not report event types.
pub(super) fn reported_event(kind: KeyEventKind, flags: u8) -> u8 {
    if flags & KITTY_EVENTS == 0 {
        return 0;
    }
    match kind {
        KeyEventKind::Press => 0,
        KeyEventKind::Repeat => 2,
        KeyEventKind::Release => 3,
    }
}

/// The kitty protocol encoding of a key under `flags`, or `None` when the legacy encoding applies.
/// `Some(vec![])` is a release the flags do not report (text keys and Enter, Tab and Backspace
/// releases need report-all-keys).
pub(super) fn kitty_key(
    code: KeyCode,
    mods: KeyModifiers,
    kind: KeyEventKind,
    flags: u8,
) -> Option<Vec<u8>> {
    if flags & (KITTY_DISAMBIGUATE | KITTY_EVENTS | KITTY_ALL_KEYS) == 0 {
        return None;
    }
    let all = flags & KITTY_ALL_KEYS != 0;
    let event = reported_event(kind, flags);
    let (code, mods) = match code {
        KeyCode::BackTab if all => (KeyCode::Tab, mods | KeyModifiers::SHIFT),
        other => (other, mods),
    };
    if kind == KeyEventKind::Release && !all && is_legacy_text(code, mods) {
        return Some(Vec::new());
    }
    if !reports_csi_u(code, mods, flags, event) {
        return None;
    }
    Some(csi_u_fields(code, mods, flags, event).encode())
}

/// Printable characters without Ctrl or Alt, and Enter, Tab and Backspace: the keys whose press
/// is typed text or a legacy control byte.
fn is_legacy_text(code: KeyCode, mods: KeyModifiers) -> bool {
    let ctrl_or_alt = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    (matches!(code, KeyCode::Char(_)) && !ctrl_or_alt)
        || matches!(code, KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace)
}

/// Whether `flags` make this key a CSI u sequence rather than its legacy encoding.
fn reports_csi_u(code: KeyCode, mods: KeyModifiers, flags: u8, event: u8) -> bool {
    let ctrl_or_alt = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let any_mod = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let disambiguate = flags & KITTY_DISAMBIGUATE != 0;
    if flags & KITTY_ALL_KEYS != 0
        && matches!(
            code,
            KeyCode::Char(_) | KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace | KeyCode::Esc
        )
    {
        return true;
    }
    match code {
        KeyCode::Esc => disambiguate || event != 0,
        KeyCode::Char(_) => ctrl_or_alt && (disambiguate || event != 0),
        KeyCode::Enter | KeyCode::Backspace => disambiguate && any_mod,
        KeyCode::Tab => disambiguate && ctrl_or_alt,
        _ => false,
    }
}

/// The fields of a CSI u sequence: key code, shifted code, modifiers, event type and text.
struct CsiU {
    key: u32,
    shifted: Option<u32>,
    mods: KeyModifiers,
    event: u8,
    text: Option<char>,
}

impl CsiU {
    fn plain(key: u32, mods: KeyModifiers, event: u8) -> Self {
        Self {
            key,
            shifted: None,
            mods,
            event,
            text: None,
        }
    }

    /// `CSI key[:shifted][;mods[:event]][;text] u`, leaving out fields at their defaults.
    fn encode(&self) -> Vec<u8> {
        let mut out = format!("\x1b[{}", self.key);
        if let Some(shifted) = self.shifted {
            out.push_str(&format!(":{shifted}"));
        }
        let field = param_field(self.mods, self.event);
        if field.is_some() || self.text.is_some() {
            out.push(';');
            out.extend(field);
        }
        if let Some(text) = self.text {
            out.push_str(&format!(";{}", u32::from(text)));
        }
        out.push('u');
        out.into_bytes()
    }
}

fn csi_u_fields(code: KeyCode, mods: KeyModifiers, flags: u8, event: u8) -> CsiU {
    match code {
        KeyCode::Char(c) => char_fields(c, mods, flags, event),
        KeyCode::Enter => CsiU::plain(13, mods, event),
        KeyCode::Tab => CsiU::plain(9, mods, event),
        KeyCode::Backspace => CsiU::plain(127, mods, event),
        _ => CsiU::plain(27, mods, event),
    }
}

/// A character key: its lowercase code point, the shifted code point under the alternate-keys
/// flag, and its associated text under the text flag.
fn char_fields(c: char, mods: KeyModifiers, flags: u8, event: u8) -> CsiU {
    let upper = c.is_ascii_uppercase();
    let ctrl_or_alt = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let mods = if upper {
        mods | KeyModifiers::SHIFT
    } else {
        mods
    };
    let all = flags & KITTY_ALL_KEYS != 0;
    let shifted = (upper && flags & KITTY_ALTERNATES != 0).then_some(u32::from(c));
    let text = (all && flags & KITTY_TEXT != 0 && !ctrl_or_alt && event != 3).then_some(c);
    CsiU {
        key: u32::from(c.to_ascii_lowercase()),
        shifted,
        mods,
        event,
        text,
    }
}
