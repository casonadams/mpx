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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellStyle {
    /// Reverse video (SGR 7): the terminal swaps foreground and background when drawing.
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    /// Faint (SGR 2): the terminal draws the text dimmer than its color.
    pub faint: bool,
    pub italic: bool,
    /// Underline style (SGR 4, 4:n, 21).
    pub underline: Underline,
    /// Underline color (SGR 58), `None` for the foreground color.
    pub underline_color: Option<Color>,
    /// OSC 8 hyperlink the cell's text links to.
    pub link: Option<LinkId>,
}

impl CellStyle {
    pub fn is_underlined(&self) -> bool {
        self.underline != Underline::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub style: CellStyle,
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
        underline: Underline::None,
        underline_color: None,
        link: None,
    },
};

impl Default for Cell {
    fn default() -> Self {
        BLANK
    }
}
