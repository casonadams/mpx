use std::io::{self, Write};

use crate::vt::{CellStyle, Color, LinkId, Underline};

/// One SGR sequence that fully specifies `style`, starting from a reset so no attribute leaks.
pub fn write_sgr<W: Write>(out: &mut W, style: &CellStyle) -> io::Result<()> {
    out.write_all(b"\x1b[0")?;
    if style.bold {
        out.write_all(b";1")?;
    }
    if style.faint {
        out.write_all(b";2")?;
    }
    if style.italic {
        out.write_all(b";3")?;
    }
    match style.underline {
        Underline::None => {}
        Underline::Single => out.write_all(b";4")?,
        Underline::Double => out.write_all(b";4:2")?,
        Underline::Curly => out.write_all(b";4:3")?,
        Underline::Dotted => out.write_all(b";4:4")?,
        Underline::Dashed => out.write_all(b";4:5")?,
    }
    if style.reverse {
        out.write_all(b";7")?;
    }
    if let Some(fg) = style.fg {
        write_color(out, fg, 30)?;
    }
    if let Some(bg) = style.bg {
        write_color(out, bg, 40)?;
    }
    if let Some(color) = style.underline_color {
        write_underline_color(out, color)?;
    }
    out.write_all(b"m")
}

/// OSC 8: starts the hyperlink `link`, or ends the current one when `None`.
pub(super) fn write_link<W: Write>(out: &mut W, link: Option<LinkId>) -> io::Result<()> {
    match link {
        Some(id) => id.with_body(|body| write!(out, "\x1b]8;{body}\x1b\\")),
        None => out.write_all(b"\x1b]8;;\x1b\\"),
    }
}

/// The colours SGR 30-37 and 90-97 name, in palette order.
const NAMED_COLORS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::White,
    Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    Color::BrightWhite,
];

/// An underline colour, in the colon forms terminals read as SGR 58 (`58:5:n`, `58:2::r:g:b`).
fn write_underline_color<W: Write>(out: &mut W, color: Color) -> io::Result<()> {
    match color {
        Color::Rgb(r, g, b) => write!(out, ";58:2::{r}:{g}:{b}"),
        Color::Indexed(i) => write!(out, ";58:5:{i}"),
        named => {
            let index = NAMED_COLORS.iter().position(|c| *c == named).unwrap_or(0);
            write!(out, ";58:5:{index}")
        }
    }
}

/// `base` is 30 for foreground and 40 for background; bright colors use the 90/100 range.
fn write_color<W: Write>(out: &mut W, color: Color, base: u8) -> io::Result<()> {
    let code = match color {
        Color::Indexed(i) => return write!(out, ";{};5;{i}", base + 8),
        Color::Rgb(r, g, b) => return write!(out, ";{};2;{r};{g};{b}", base + 8),
        Color::Black => base,
        Color::Red => base + 1,
        Color::Green => base + 2,
        Color::Yellow => base + 3,
        Color::Blue => base + 4,
        Color::Magenta => base + 5,
        Color::Cyan => base + 6,
        Color::White => base + 7,
        Color::BrightBlack => base + 60,
        Color::BrightRed => base + 61,
        Color::BrightGreen => base + 62,
        Color::BrightYellow => base + 63,
        Color::BrightBlue => base + 64,
        Color::BrightMagenta => base + 65,
        Color::BrightCyan => base + 66,
        Color::BrightWhite => base + 67,
    };
    write!(out, ";{code}")
}
