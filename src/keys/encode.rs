use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::csi::{ESC, cursor_key, function_key, letter_key, modifier_param, tilde_key};
use super::kitty::{kitty_key, reported_event};
use super::spec::{KeySpec, normalize};
use super::{KeyPolicy, TermModes};
use crate::config::ExtendedKeys;

const CR: u8 = b'\r';
const HT: u8 = b'\t';
const DEL: u8 = 0x7f;
const BS: u8 = 0x08;

impl KeySpec {
    pub fn encode(&self, modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
        encode_parts(self.code, self.mods, KeyEventKind::Press, modes, policy)
    }
}

/// xterm encoding of a crossterm key event, honouring the pane's input modes and the key policy.
pub fn encode_key(ev: &KeyEvent, modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
    encode_parts(ev.code, ev.modifiers, ev.kind, modes, policy)
}

/// tmux `send-keys` semantics: each argument is a key name when it parses as one, otherwise literal text.
pub fn encode_key_names(names: &[String], modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
    names
        .iter()
        .flat_map(|name| encode_name(name, modes, policy))
        .collect()
}

fn encode_name(name: &str, modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
    if name.chars().count() > 1
        && let Ok(spec) = KeySpec::parse(name)
    {
        return spec.encode(modes, policy);
    }
    name.as_bytes().to_vec()
}

/// `ESC[I` / `ESC[O`.
pub fn focus_event(focused: bool) -> &'static [u8] {
    if focused { b"\x1b[I" } else { b"\x1b[O" }
}

fn encode_parts(
    code: KeyCode,
    mods: KeyModifiers,
    kind: KeyEventKind,
    modes: &TermModes,
    policy: KeyPolicy,
) -> Vec<u8> {
    let modes = effective_modes(modes, policy);
    if let Some(bytes) = kitty_key(code, mods, kind, modes.kitty_keys) {
        return bytes;
    }
    if let Some(bytes) = modify_other_keys(code, mods, &modes) {
        return bytes;
    }
    let event = reported_event(kind, modes.kitty_keys);
    let (code, mods) = normalize(code, mods);
    let keyed = if policy.xterm_keys || event != 0 {
        mods
    } else {
        KeyModifiers::NONE
    };
    match code {
        KeyCode::Char(c) => encode_char(c, mods),
        KeyCode::Enter if mods.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => vec![ESC, CR],
        KeyCode::Enter => vec![CR],
        KeyCode::Tab => prefix_alt(mods, vec![HT]),
        KeyCode::Esc => prefix_alt(mods, vec![ESC]),
        KeyCode::Backspace if mods.contains(KeyModifiers::CONTROL) => prefix_alt(mods, vec![BS]),
        KeyCode::Backspace => prefix_alt(mods, vec![DEL]),
        KeyCode::BackTab => letter_key(b'[', b'Z', keyed, event),
        KeyCode::Up => cursor_key(b'A', keyed, event, modes.app_cursor),
        KeyCode::Down => cursor_key(b'B', keyed, event, modes.app_cursor),
        KeyCode::Right => cursor_key(b'C', keyed, event, modes.app_cursor),
        KeyCode::Left => cursor_key(b'D', keyed, event, modes.app_cursor),
        KeyCode::Home => cursor_key(b'H', keyed, event, modes.app_cursor),
        KeyCode::End => cursor_key(b'F', keyed, event, modes.app_cursor),
        KeyCode::Insert => tilde_key(2, keyed, event),
        KeyCode::Delete => tilde_key(3, keyed, event),
        KeyCode::PageUp => tilde_key(5, keyed, event),
        KeyCode::PageDown => tilde_key(6, keyed, event),
        KeyCode::F(n) => function_key(n, keyed, event),
        _ => Vec::new(),
    }
}

/// The modes keys are encoded with under `policy`: `off` ignores what the pane asked for, and
/// `always` sends modified keys as modifyOtherKeys unless the pane asked for the kitty protocol.
fn effective_modes(modes: &TermModes, policy: KeyPolicy) -> TermModes {
    match policy.extended {
        ExtendedKeys::Off => TermModes {
            modify_other_keys: 0,
            kitty_keys: 0,
            ..*modes
        },
        ExtendedKeys::On => *modes,
        ExtendedKeys::Always if modes.kitty_keys == 0 => TermModes {
            modify_other_keys: modes.modify_other_keys.max(2),
            ..*modes
        },
        ExtendedKeys::Always => *modes,
    }
}

/// The xterm modifyOtherKeys form of a key a pane asked for: Ctrl-modified characters and
/// modified Enter, Tab, Backspace and Esc. `None` when the legacy encoding applies.
fn modify_other_keys(code: KeyCode, mods: KeyModifiers, modes: &TermModes) -> Option<Vec<u8>> {
    let any_mod = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    if modes.modify_other_keys == 0 {
        return None;
    }
    let key = match code {
        KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) => Some(u32::from(c)),
        KeyCode::Enter if any_mod => Some(13),
        KeyCode::Tab if any_mod => Some(9),
        KeyCode::Backspace if any_mod => Some(127),
        KeyCode::Esc if any_mod => Some(27),
        _ => None,
    };
    key.map(|key| format!("\x1b[27;{};{key}~", modifier_param(mods)).into_bytes())
}

fn encode_char(c: char, mods: KeyModifiers) -> Vec<u8> {
    let body = match ctrl_byte(c) {
        Some(b) if mods.contains(KeyModifiers::CONTROL) => vec![b],
        _ => c.to_string().into_bytes(),
    };
    prefix_alt(mods, body)
}

fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' | 'A'..='Z' => Some(c.to_ascii_lowercase() as u8 - b'a' + 1),
        ' ' | '@' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(DEL),
        _ => None,
    }
}

fn prefix_alt(mods: KeyModifiers, body: Vec<u8>) -> Vec<u8> {
    if !mods.contains(KeyModifiers::ALT) {
        return body;
    }
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(ESC);
    out.extend(body);
    out
}
