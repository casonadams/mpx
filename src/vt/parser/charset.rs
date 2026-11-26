#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Charset {
    #[default]
    Ascii,
    DecSpecialGraphics,
}

impl Charset {
    fn from_final(byte: u8) -> Self {
        if byte == b'0' {
            Self::DecSpecialGraphics
        } else {
            Self::Ascii
        }
    }

    fn translate(self, c: char) -> char {
        match self {
            Self::Ascii => c,
            Self::DecSpecialGraphics => dec_special_graphic(c),
        }
    }
}

/// The VT100 line-drawing set (xterm ctlseqs, "DEC Special Character and Line Drawing Set").
fn dec_special_graphic(c: char) -> char {
    match c {
        '_' => ' ',
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Charsets {
    g0: Charset,
    g1: Charset,
    g1_active: bool,
}

impl Charsets {
    pub(super) fn designate(&mut self, intermediates: &[u8], final_byte: u8) -> bool {
        let slot = match intermediates {
            b"(" => &mut self.g0,
            b")" => &mut self.g1,
            _ => return false,
        };
        *slot = Charset::from_final(final_byte);
        true
    }

    pub(super) fn shift_g1(&mut self, active: bool) {
        self.g1_active = active;
    }

    pub(super) fn translate(&self, c: char) -> char {
        let active = if self.g1_active { self.g1 } else { self.g0 };
        active.translate(c)
    }
}
