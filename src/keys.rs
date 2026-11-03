//! Key encoding shared by the TUI (crossterm events), `send-keys` (tmux key names) and key binding specs.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use serde::{Deserialize, Serialize};
use crate::config::{Config, ExtendedKeys};

const ESC: u8 = 0x1b;
const CR: u8 = b'\r';
const HT: u8 = b'\t';
const DEL: u8 = 0x7f;
const BS: u8 = 0x08;

/// Mouse tracking mode requested by an application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseTracking {
    #[default]
    None,
    /// `CSI ? 1000 h`: report clicks only.
    X10,
    /// `CSI ? 1002 h`: report clicks and button-down drags.
    ButtonEvent,
    /// `CSI ? 1003 h`: report all mouse movement.
    AnyEvent,
}

/// Terminal input modes requested by the program running inside a pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TermModes {
    /// DECCKM (`CSI ? 1 h`): arrows/Home/End use SS3 (`ESC O A`).
    pub app_cursor: bool,
    /// `CSI ? 2004 h`.
    pub bracketed_paste: bool,
    /// `CSI ? 1004 h`.
    pub focus_events: bool,
    /// Mouse tracking protocol.
    pub mouse_tracking: MouseTracking,
    /// `CSI ? 1006 h`: SGR extended coordinates format.
    pub mouse_sgr: bool,
    /// xterm modifyOtherKeys level the program set (`CSI > 4 ; n m`). Levels 1 and 2 send modified
    /// keys as `CSI 27 ; m ; code ~`.
    pub modify_other_keys: u8,
    /// Kitty keyboard protocol flags the program set. Bit 1 (disambiguate) sends ambiguous keys as
    /// `CSI code ; m u`.
    pub kitty_keys: u8,
}

/// What the options allow for keys sent to panes: `extended-keys` and `xterm-keys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPolicy {
    pub extended: ExtendedKeys,
    pub xterm_keys: bool,
}

impl KeyPolicy {
    pub fn of(config: &Config) -> Self {
        Self {
            extended: config.terminal.extended_keys,
            xterm_keys: config.terminal.xterm_keys,
        }
    }
}

/// Kitty keyboard protocol flag 1: keys that are ambiguous in legacy encoding are sent as CSI u.
const KITTY_DISAMBIGUATE: u8 = 0b1;

/// A key with modifiers, parsed from tmux notation (`C-b`, `M-x`, `S-Tab`, `F5`, `h`, `H`, `Space`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeySpec {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeySpec {
    pub fn parse(spec: &str) -> Result<Self, String> {
        if spec.is_empty() {
            return Err("empty key name".to_owned());
        }
        let (mut mods, base) = strip_modifiers(spec);
        let (code, implied) = parse_base(base).ok_or_else(|| format!("unknown key: {spec}"))?;
        mods |= implied;
        let code = match code {
            KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            other => other,
        };
        Ok(Self { code, mods })
    }

    /// True when a crossterm event is this key (SHIFT is ignored for printable chars, case is significant).
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        normalize(self.code, self.mods) == normalize(ev.code, ev.modifiers)
    }

    /// Canonical tmux notation, e.g. `C-b`, `M-Left`, `"`.
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (flag, prefix) in [
            (KeyModifiers::CONTROL, "C-"),
            (KeyModifiers::ALT, "M-"),
            (KeyModifiers::SHIFT, "S-"),
        ] {
            if self.mods.contains(flag)
                && !(flag == KeyModifiers::SHIFT && self.code == KeyCode::BackTab)
            {
                out.push_str(prefix);
            }
        }
        out.push_str(&base_name(self.code));
        out
    }

    pub fn encode(&self, modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
        encode_parts(self.code, self.mods, modes, policy)
    }
}

const NAMED_KEYS: &[(&str, KeyCode)] = &[
    ("Enter", KeyCode::Enter),
    ("Escape", KeyCode::Esc),
    ("Esc", KeyCode::Esc),
    ("Tab", KeyCode::Tab),
    ("BTab", KeyCode::BackTab),
    ("Space", KeyCode::Char(' ')),
    ("BSpace", KeyCode::Backspace),
    ("Up", KeyCode::Up),
    ("Down", KeyCode::Down),
    ("Left", KeyCode::Left),
    ("Right", KeyCode::Right),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PageUp", KeyCode::PageUp),
    ("PgUp", KeyCode::PageUp),
    ("PPage", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("PgDn", KeyCode::PageDown),
    ("NPage", KeyCode::PageDown),
    ("IC", KeyCode::Insert),
    ("Insert", KeyCode::Insert),
    ("DC", KeyCode::Delete),
    ("Delete", KeyCode::Delete),
];

fn prefix_modifier(prefix: &str) -> Option<KeyModifiers> {
    match prefix {
        "C-" => Some(KeyModifiers::CONTROL),
        "M-" => Some(KeyModifiers::ALT),
        "S-" => Some(KeyModifiers::SHIFT),
        _ => None,
    }
}

/// Consumes leading `C-`/`M-`/`S-` prefixes; a prefix only counts when a key follows it.
fn strip_modifiers(mut rest: &str) -> (KeyModifiers, &str) {
    let mut mods = KeyModifiers::NONE;
    while rest.len() > 2 {
        let Some(m) = rest.get(..2).and_then(prefix_modifier) else {
            break;
        };
        mods |= m;
        rest = &rest[2..];
    }
    (mods, rest)
}

/// Parses the unprefixed key: `^x` alias, a single char, or a named key.
fn parse_base(base: &str) -> Option<(KeyCode, KeyModifiers)> {
    let mut chars = base.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some('^'), Some(c), None) => Some((KeyCode::Char(c), KeyModifiers::CONTROL)),
        (Some(c), None, None) => Some((KeyCode::Char(c), KeyModifiers::NONE)),
        _ => named_key(base).map(|code| {
            let implied = if code == KeyCode::BackTab {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            (code, implied)
        }),
    }
}

fn named_key(name: &str) -> Option<KeyCode> {
    if let Some(&(_, code)) = NAMED_KEYS.iter().find(|(n, _)| *n == name) {
        return Some(code);
    }
    let n: u8 = name.strip_prefix('F')?.parse().ok()?;
    (1..=12).contains(&n).then_some(KeyCode::F(n))
}

/// Canonical event form: SHIFT folded into printable chars, BackTab without SHIFT, ctrl letters lowercased.
fn normalize(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
    match code {
        KeyCode::Char(c) => (
            KeyCode::Char(normalize_char(c, mods)),
            mods & !KeyModifiers::SHIFT,
        ),
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => normalize(KeyCode::BackTab, mods),
        KeyCode::BackTab => (code, mods & !KeyModifiers::SHIFT),
        _ => (code, mods),
    }
}

fn normalize_char(c: char, mods: KeyModifiers) -> char {
    let c = if mods.contains(KeyModifiers::SHIFT) {
        c.to_ascii_uppercase()
    } else {
        c
    };
    if mods.contains(KeyModifiers::CONTROL) {
        c.to_ascii_lowercase()
    } else {
        c
    }
}

fn base_name(code: KeyCode) -> String {
    let name = match code {
        KeyCode::Char(' ') => "Space",
        KeyCode::Char(c) => return c.to_string(),
        KeyCode::Enter => "Enter",
        KeyCode::Esc => "Escape",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => "BTab",
        KeyCode::Backspace => "BSpace",
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PageUp",
        KeyCode::PageDown => "PageDown",
        KeyCode::Insert => "IC",
        KeyCode::Delete => "DC",
        KeyCode::F(n) => return format!("F{n}"),
        other => return format!("{other:?}"),
    };
    name.to_owned()
}

/// xterm encoding of a crossterm key event, honouring the pane's input modes and the key policy.
pub fn encode_key(ev: &KeyEvent, modes: &TermModes, policy: KeyPolicy) -> Vec<u8> {
    encode_parts(ev.code, ev.modifiers, modes, policy)
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

/// Wraps text in `ESC[200~ … ESC[201~` when the program enabled bracketed paste; strips embedded end markers.
pub fn wrap_paste(text: &str, modes: &TermModes) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !modes.bracketed_paste {
        return text.into_bytes();
    }
    bracket(&text)
}

/// Bytes that paste a paste buffer. Line feeds become `separator` when given, else carriage
/// returns, which `raw` keeps as they are. Bracketed when `bracketed` is set or the program
/// enabled bracketed paste.
pub fn paste_buffer_bytes(
    text: &str,
    separator: Option<&str>,
    raw: bool,
    bracketed: bool,
    modes: &TermModes,
) -> Vec<u8> {
    let body = match separator {
        Some(separator) => text.replace("\r\n", "\n").replace('\n', separator),
        None if raw => text.to_string(),
        None => text.replace("\r\n", "\r").replace('\n', "\r"),
    };
    if !(bracketed || modes.bracketed_paste) {
        return body.into_bytes();
    }
    bracket(&body)
}

fn bracket(text: &str) -> Vec<u8> {
    let body = strip_paste_end(text);
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

fn strip_paste_end(text: &str) -> String {
    let mut out = text.to_owned();
    while out.contains("\x1b[201~") {
        out = out.replace("\x1b[201~", "");
    }
    out
}

/// `ESC[I` / `ESC[O`.
pub fn focus_event(focused: bool) -> &'static [u8] {
    if focused { b"\x1b[I" } else { b"\x1b[O" }
}

fn encode_parts(
    code: KeyCode,
    mods: KeyModifiers,
    modes: &TermModes,
    policy: KeyPolicy,
) -> Vec<u8> {
    let modes = effective_modes(modes, policy);
    if let Some(bytes) = extended_key(code, mods, &modes) {
        return bytes;
    }
    let (code, mods) = normalize(code, mods);
    let keyed = if policy.xterm_keys {
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
        KeyCode::BackTab => csi_key(b'Z', keyed),
        KeyCode::Up => cursor_key(b'A', keyed, modes.app_cursor),
        KeyCode::Down => cursor_key(b'B', keyed, modes.app_cursor),
        KeyCode::Right => cursor_key(b'C', keyed, modes.app_cursor),
        KeyCode::Left => cursor_key(b'D', keyed, modes.app_cursor),
        KeyCode::Home => cursor_key(b'H', keyed, modes.app_cursor),
        KeyCode::End => cursor_key(b'F', keyed, modes.app_cursor),
        KeyCode::Insert => tilde_key(2, keyed),
        KeyCode::Delete => tilde_key(3, keyed),
        KeyCode::PageUp => tilde_key(5, keyed),
        KeyCode::PageDown => tilde_key(6, keyed),
        KeyCode::F(n) => function_key(n, keyed),
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

/// The CSI form of a key a pane asked for: the kitty protocol for the keys it disambiguates, or
/// xterm modifyOtherKeys for Ctrl-modified characters and modified Enter, Tab, Backspace and Esc.
/// `None` when the legacy encoding applies.
fn extended_key(code: KeyCode, mods: KeyModifiers, modes: &TermModes) -> Option<Vec<u8>> {
    let any_mod = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let ctrl_or_alt = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    if modes.kitty_keys & KITTY_DISAMBIGUATE != 0 {
        let key = match code {
            KeyCode::Esc => Some(27),
            KeyCode::Enter if any_mod => Some(13),
            KeyCode::Tab if ctrl_or_alt => Some(9),
            KeyCode::Backspace if any_mod => Some(127),
            KeyCode::Char(c) if ctrl_or_alt => Some(u32::from(c.to_ascii_lowercase())),
            _ => None,
        };
        if let Some(key) = key {
            let m = modifier_param(mods);
            let csi = if m == 1 {
                format!("\x1b[{key}u")
            } else {
                format!("\x1b[{key};{m}u")
            };
            return Some(csi.into_bytes());
        }
    }
    if modes.modify_other_keys > 0 {
        let key = match code {
            KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) => Some(u32::from(c)),
            KeyCode::Enter if any_mod => Some(13),
            KeyCode::Tab if any_mod => Some(9),
            KeyCode::Backspace if any_mod => Some(127),
            KeyCode::Esc if any_mod => Some(27),
            _ => None,
        };
        if let Some(key) = key {
            return Some(format!("\x1b[27;{};{key}~", modifier_param(mods)).into_bytes());
        }
    }
    None
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

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(mods: KeyModifiers) -> u8 {
    let bit = |flag: KeyModifiers, weight: u8| if mods.contains(flag) { weight } else { 0 };
    1 + bit(KeyModifiers::SHIFT, 1) + bit(KeyModifiers::ALT, 2) + bit(KeyModifiers::CONTROL, 4)
}

/// `CSI <letter>`, or `CSI 1;m <letter>` when modified.
fn csi_key(letter: u8, mods: KeyModifiers) -> Vec<u8> {
    match modifier_param(mods) {
        1 => vec![ESC, b'[', letter],
        m => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
    }
}

/// `ESC O <letter>`, or `CSI 1;m <letter>` when modified.
fn ss3_key(letter: u8, mods: KeyModifiers) -> Vec<u8> {
    match modifier_param(mods) {
        1 => vec![ESC, b'O', letter],
        m => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
    }
}

fn cursor_key(letter: u8, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    if app_cursor {
        ss3_key(letter, mods)
    } else {
        csi_key(letter, mods)
    }
}

/// `CSI n ~`, or `CSI n;m ~` when modified.
fn tilde_key(n: u8, mods: KeyModifiers) -> Vec<u8> {
    match modifier_param(mods) {
        1 => format!("\x1b[{n}~").into_bytes(),
        m => format!("\x1b[{n};{m}~").into_bytes(),
    }
}

fn function_key(n: u8, mods: KeyModifiers) -> Vec<u8> {
    const SS3_LETTERS: [u8; 4] = *b"PQRS";
    const TILDE_CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
    match n {
        1..=4 => ss3_key(SS3_LETTERS[usize::from(n - 1)], mods),
        5..=12 => tilde_key(TILDE_CODES[usize::from(n - 5)], mods),
        _ => Vec::new(),
    }
}

pub fn encode_mouse_event(
    modes: &TermModes,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    match modes.mouse_tracking {
        MouseTracking::None => None,
        MouseTracking::X10 => match kind {
            MouseEventKind::Down(_) => {
                if modes.mouse_sgr {
                    encode_sgr_mouse(kind, col, row, mods)
                } else {
                    encode_x10_mouse(kind, col, row, mods)
                }
            }
            _ => None,
        },
        MouseTracking::ButtonEvent => match kind {
            MouseEventKind::Down(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                if modes.mouse_sgr {
                    encode_sgr_mouse(kind, col, row, mods)
                } else {
                    encode_x10_mouse(kind, col, row, mods)
                }
            }
            _ => None,
        },
        MouseTracking::AnyEvent => {
            if modes.mouse_sgr {
                encode_sgr_mouse(kind, col, row, mods)
            } else {
                encode_x10_mouse(kind, col, row, mods)
            }
        }
    }
}

pub fn encode_sgr_mouse(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    let (mut btn, release) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Down(MouseButton::Middle) => (1, false),
        MouseEventKind::Down(MouseButton::Right) => (2, false),
        MouseEventKind::Up(MouseButton::Left) => (0, true),
        MouseEventKind::Up(MouseButton::Middle) => (1, true),
        MouseEventKind::Up(MouseButton::Right) => (2, true),
        MouseEventKind::Drag(MouseButton::Left) => (32, false),
        MouseEventKind::Drag(MouseButton::Middle) => (33, false),
        MouseEventKind::Drag(MouseButton::Right) => (34, false),
        MouseEventKind::Moved => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
    };

    if mods.contains(KeyModifiers::SHIFT) {
        btn += 4;
    }
    if mods.contains(KeyModifiers::ALT) {
        btn += 8;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        btn += 16;
    }

    let final_char = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{btn};{col};{row}{final_char}").into_bytes())
}

pub fn encode_x10_mouse(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    let (mut btn, release) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Down(MouseButton::Middle) => (1, false),
        MouseEventKind::Down(MouseButton::Right) => (2, false),
        MouseEventKind::Up(_) => (3, true),
        MouseEventKind::Drag(MouseButton::Left) => (32, false),
        MouseEventKind::Drag(MouseButton::Middle) => (33, false),
        MouseEventKind::Drag(MouseButton::Right) => (34, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return None,
    };
    if release {
        btn = 3;
    }
    if mods.contains(KeyModifiers::SHIFT) {
        btn += 4;
    }
    if mods.contains(KeyModifiers::ALT) {
        btn += 8;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        btn += 16;
    }
    let col = col.clamp(1, 223) as u8;
    let row = row.clamp(1, 223) as u8;
    Some(vec![0x1b, b'[', b'M', 32 + btn, 32 + col, 32 + row])
}
