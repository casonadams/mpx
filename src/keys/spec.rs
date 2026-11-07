use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
}

/// Tmux names of the named keys. The first entry for a code is its canonical name.
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

/// Consumes leading `C-`/`M-`/`S-` prefixes; a prefix only counts when a key follows it.
fn strip_modifiers(mut rest: &str) -> (KeyModifiers, &str) {
    let mut mods = KeyModifiers::NONE;
    while rest.len() > 2 {
        let m = match rest.get(..2) {
            Some("C-") => KeyModifiers::CONTROL,
            Some("M-") => KeyModifiers::ALT,
            Some("S-") => KeyModifiers::SHIFT,
            _ => break,
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
pub(super) fn normalize(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
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
    match code {
        KeyCode::Char(c) if c != ' ' => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => match NAMED_KEYS.iter().find(|(_, named)| *named == code) {
            Some((name, _)) => (*name).to_owned(),
            None => format!("{code:?}"),
        },
    }
}
