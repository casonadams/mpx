use std::io;

use unicode_width::UnicodeWidthChar;

use crate::vt::combining::Combining;
use crate::vt::link::LinkId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    /// The palette slot (0-15) of a named ANSI colour, `None` for indexed and RGB colours.
    pub fn named_slot(self) -> Option<u8> {
        Some(match self {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::White => 7,
            Color::BrightBlack => 8,
            Color::BrightRed => 9,
            Color::BrightGreen => 10,
            Color::BrightYellow => 11,
            Color::BrightBlue => 12,
            Color::BrightMagenta => 13,
            Color::BrightCyan => 14,
            Color::BrightWhite => 15,
            Color::Indexed(_) | Color::Rgb(..) => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Copy, Eq, Default)]
pub struct CellStyle {
    /// Reverse video (SGR 7): the terminal swaps foreground and background when drawing.
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    /// Faint (SGR 2): the terminal draws the text dimmer than its color.
    pub faint: bool,
    pub italic: bool,
    /// Blink (SGR 5), conceal (SGR 8) and strikethrough (SGR 9): drawn by the terminal.
    pub blink: bool,
    pub conceal: bool,
    pub strike: bool,
    /// Underline style (SGR 4, 4:n, 21).
    pub underline: Underline,
    /// Underline color (SGR 58), `None` for the foreground color.
    pub underline_color: Option<Color>,
    /// OSC 8 hyperlink the cell's text links to.
    pub link: Option<LinkId>,
}

impl PartialEq for CellStyle {
    /// Field by field, inlined: grids compare styles per cell when scrolling rows into history.
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let Self {
            reverse,
            fg,
            bg,
            bold,
            faint,
            italic,
            blink,
            conceal,
            strike,
            underline,
            underline_color,
            link,
        } = *self;
        [reverse, bold, faint, italic, blink, conceal, strike]
            == [
                other.reverse,
                other.bold,
                other.faint,
                other.italic,
                other.blink,
                other.conceal,
                other.strike,
            ]
            && underline == other.underline
            && fg == other.fg
            && bg == other.bg
            && underline_color == other.underline_color
            && link == other.link
    }
}

impl CellStyle {
    pub fn is_underlined(&self) -> bool {
        self.underline != Underline::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The character, or `WIDE_SPACER` for the right half of a wide character.
    pub c: char,
    pub style: CellStyle,
    /// Zero-width characters drawn on `c`.
    pub combining: Option<Combining>,
}

/// What the cell right of a two-column character holds: it takes the column, and draws nothing.
pub const WIDE_SPACER: char = '\0';

/// Columns `c` takes on a terminal: 2 for wide characters, 0 for combining and other zero-width
/// characters, `None` for control characters, which are not printed.
#[inline]
pub fn char_width(c: char) -> Option<usize> {
    if (' '..'\x7f').contains(&c) {
        return Some(1);
    }
    c.width()
}

impl Cell {
    /// The right half of a wide character.
    pub fn is_wide_spacer(&self) -> bool {
        self.c == WIDE_SPACER
    }

    /// The left half of a two-column character: wide by its Unicode width, or by the emoji
    /// presentation drawn on it. A `WIDE_SPACER` follows it in the row.
    pub fn is_wide(&self) -> bool {
        !self.is_wide_spacer()
            && (char_width(self.c) == Some(2) || self.combining.is_some_and(Combining::widens))
    }

    /// A printable ASCII character with nothing drawn on it: one byte, one column.
    pub fn is_plain(&self) -> bool {
        (' '..'\x7f').contains(&self.c) && self.combining.is_none()
    }

    /// Appends the text the cell draws: nothing for a wide spacer.
    pub fn push_text(&self, out: &mut String) {
        if self.is_wide_spacer() {
            return;
        }
        out.push(self.c);
        if let Some(marks) = self.combining {
            marks.with_str(|s| out.push_str(s));
        }
    }

    /// Writes the text the cell draws as UTF-8: nothing for a wide spacer.
    pub fn write_text<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        if self.is_wide_spacer() {
            return Ok(());
        }
        let mut utf8 = [0u8; 4];
        out.write_all(self.c.encode_utf8(&mut utf8).as_bytes())?;
        match self.combining {
            Some(marks) => marks.with_str(|s| out.write_all(s.as_bytes())),
            None => Ok(()),
        }
    }
}

/// A blank cell in default attributes: what a history row holds past its stored end.
pub(super) const BLANK: Cell = Cell {
    c: ' ',
    style: CellStyle {
        reverse: false,
        fg: None,
        bg: None,
        bold: false,
        faint: false,
        italic: false,
        blink: false,
        conceal: false,
        strike: false,
        underline: Underline::None,
        underline_color: None,
        link: None,
    },
    combining: None,
};

impl Default for Cell {
    fn default() -> Self {
        BLANK
    }
}
