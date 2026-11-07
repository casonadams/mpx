use vte::Params;

use super::VtHandler;
use crate::vt::grid::{CellStyle, Color, Underline};

const ANSI_COLORS: [Color; 16] = [
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

impl VtHandler {
    pub(super) fn handle_sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.grid.current_style = CellStyle::default();
            return;
        }

        let mut iter = params.iter();
        while let Some(sub) = iter.next() {
            let code = sub[0];
            let style = &mut self.grid.current_style;
            match code {
                38 => {
                    if let Some(color) = parse_extended_color(sub, &mut iter) {
                        style.fg = Some(color);
                    }
                }
                48 => {
                    if let Some(color) = parse_extended_color(sub, &mut iter) {
                        style.bg = Some(color);
                    }
                }
                58 => {
                    if let Some(color) = underline_color(sub, &mut iter) {
                        style.underline_color = Some(color);
                    }
                }
                _ => apply_sgr_code(style, code, sub),
            }
        }
    }
}

/// The colour a 58 underline selector sets, in colon or semicolon form.
fn underline_color<'a>(sub: &[u16], iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    if sub.len() > 1 {
        colon_extended_color(sub)
    } else {
        semicolon_extended_color(iter)
    }
}

/// The underline style of SGR 4 (`4:n`); a bare 4 is a single underline, `4:0` none.
fn underline_style(sub: &[u16]) -> Underline {
    match sub.get(1) {
        None | Some(1) => Underline::Single,
        Some(0) => Underline::None,
        Some(2) => Underline::Double,
        Some(3) => Underline::Curly,
        Some(4) => Underline::Dotted,
        Some(5) => Underline::Dashed,
        Some(_) => Underline::Single,
    }
}

/// Applies one SGR code other than the extended colour selectors (38, 48, 58).
fn apply_sgr_code(style: &mut CellStyle, code: u16, sub: &[u16]) {
    match code {
        0 | 22..=24 | 27 => reset_attribute(style, code),
        1..=4 | 7 | 21 => set_attribute(style, code, sub),
        30..=37 | 39 | 90..=97 => style.fg = basic_color(code),
        40..=47 | 49 | 100..=107 => style.bg = basic_color(code),
        59 => style.underline_color = None,
        _ => {}
    }
}

fn set_attribute(style: &mut CellStyle, code: u16, sub: &[u16]) {
    match code {
        1 => style.bold = true,
        2 => style.faint = true,
        3 => style.italic = true,
        4 => style.underline = underline_style(sub),
        7 => style.reverse = true,
        21 => style.underline = Underline::Double,
        _ => {}
    }
}

fn reset_attribute(style: &mut CellStyle, code: u16) {
    match code {
        0 => *style = CellStyle::default(),
        22 => {
            style.bold = false;
            style.faint = false;
        }
        23 => style.italic = false,
        24 => style.underline = Underline::None,
        27 => style.reverse = false,
        _ => {}
    }
}

/// Maps the basic and bright colour codes (30-37, 40-47, 90-97, 100-107) to a colour.
fn basic_color(code: u16) -> Option<Color> {
    match code {
        30..=37 => Some(ANSI_COLORS[(code - 30) as usize]),
        40..=47 => Some(ANSI_COLORS[(code - 40) as usize]),
        90..=97 => Some(ANSI_COLORS[(code - 82) as usize]),
        100..=107 => Some(ANSI_COLORS[(code - 92) as usize]),
        _ => None,
    }
}

/// Parses the colour following a 38/48 selector, in colon or semicolon form.
fn parse_extended_color<'a>(
    sub: &[u16],
    iter: &mut impl Iterator<Item = &'a [u16]>,
) -> Option<Color> {
    colon_extended_color(sub).or_else(|| semicolon_extended_color(iter))
}

fn colon_extended_color(sub: &[u16]) -> Option<Color> {
    if sub.len() >= 5 && sub[1] == 2 {
        let (r, g, b) = if sub.len() >= 6 {
            (sub[3] as u8, sub[4] as u8, sub[5] as u8)
        } else {
            (sub[2] as u8, sub[3] as u8, sub[4] as u8)
        };
        Some(Color::Rgb(r, g, b))
    } else if sub.len() >= 3 && sub[1] == 5 {
        Some(Color::Indexed(sub[2] as u8))
    } else {
        None
    }
}

fn semicolon_extended_color<'a>(iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    let sub = iter.next()?;
    match sub[0] {
        5 => iter.next().map(|c| Color::Indexed(c[0] as u8)),
        2 => match (iter.next(), iter.next(), iter.next()) {
            (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8)),
            _ => None,
        },
        _ => None,
    }
}
