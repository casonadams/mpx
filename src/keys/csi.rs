use crossterm::event::KeyModifiers;

pub(super) const ESC: u8 = 0x1b;

/// Sum of the weights of the shift, alt and ctrl bits set in `mods`.
pub(super) fn weighted_mod_bits(mods: KeyModifiers, shift: u8, alt: u8, ctrl: u8) -> u8 {
    [
        (KeyModifiers::SHIFT, shift),
        (KeyModifiers::ALT, alt),
        (KeyModifiers::CONTROL, ctrl),
    ]
    .into_iter()
    .filter(|(flag, _)| mods.contains(*flag))
    .map(|(_, weight)| weight)
    .sum()
}

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
pub(super) fn modifier_param(mods: KeyModifiers) -> u8 {
    1 + weighted_mod_bits(mods, 1, 2, 4)
}

/// The `modifiers[:event]` field of a CSI sequence, or `None` when both are at their defaults.
pub(super) fn param_field(mods: KeyModifiers, event: u8) -> Option<String> {
    match (modifier_param(mods), event) {
        (1, 0) => None,
        (m, 0) => Some(m.to_string()),
        (m, e) => Some(format!("{m}:{e}")),
    }
}

/// `ESC <intro> <letter>` (`intro` is `[` for CSI or `O` for SS3), or `CSI 1;f <letter>` when the
/// field `f` is needed.
pub(super) fn letter_key(intro: u8, letter: u8, mods: KeyModifiers, event: u8) -> Vec<u8> {
    match param_field(mods, event) {
        None => vec![ESC, intro, letter],
        Some(f) => format!("\x1b[1;{f}{}", char::from(letter)).into_bytes(),
    }
}

pub(super) fn cursor_key(letter: u8, mods: KeyModifiers, event: u8, app_cursor: bool) -> Vec<u8> {
    letter_key(if app_cursor { b'O' } else { b'[' }, letter, mods, event)
}

/// `CSI n ~`, or `CSI n;f ~` when the field `f` is needed.
pub(super) fn tilde_key(n: u8, mods: KeyModifiers, event: u8) -> Vec<u8> {
    let field = param_field(mods, event).map_or_else(String::new, |f| format!(";{f}"));
    format!("\x1b[{n}{field}~").into_bytes()
}

pub(super) fn function_key(n: u8, mods: KeyModifiers, event: u8) -> Vec<u8> {
    const SS3_LETTERS: [u8; 4] = *b"PQRS";
    const TILDE_CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
    match n {
        1..=4 => letter_key(b'O', SS3_LETTERS[usize::from(n - 1)], mods, event),
        5..=12 => tilde_key(TILDE_CODES[usize::from(n - 5)], mods, event),
        _ => Vec::new(),
    }
}
