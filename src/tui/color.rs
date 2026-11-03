use crate::vt::Color as VtColor;

/// The 16 ANSI colours as RGB: 0-7 normal, 8-15 bright.
const ANSI_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 49, 49),
    (13, 188, 121),
    (229, 229, 16),
    (36, 114, 200),
    (188, 63, 188),
    (17, 168, 205),
    (229, 229, 229),
    (102, 102, 102),
    (241, 76, 76),
    (35, 209, 139),
    (245, 245, 67),
    (59, 142, 234),
    (214, 112, 214),
    (41, 184, 219),
    (255, 255, 255),
];

/// Position of a named ANSI colour in [`ANSI_RGB`], `None` for indexed and RGB colours.
fn named_index(color: VtColor) -> Option<usize> {
    Some(match color {
        VtColor::Black => 0,
        VtColor::Red => 1,
        VtColor::Green => 2,
        VtColor::Yellow => 3,
        VtColor::Blue => 4,
        VtColor::Magenta => 5,
        VtColor::Cyan => 6,
        VtColor::White => 7,
        VtColor::BrightBlack => 8,
        VtColor::BrightRed => 9,
        VtColor::BrightGreen => 10,
        VtColor::BrightYellow => 11,
        VtColor::BrightBlue => 12,
        VtColor::BrightMagenta => 13,
        VtColor::BrightCyan => 14,
        VtColor::BrightWhite => 15,
        VtColor::Indexed(_) | VtColor::Rgb(..) => return None,
    })
}

pub fn color_to_rgb(color: VtColor) -> (u8, u8, u8) {
    match color {
        VtColor::Indexed(i) => indexed_to_rgb(i),
        VtColor::Rgb(r, g, b) => (r, g, b),
        named => named_index(named).map_or((0, 0, 0), |i| ANSI_RGB[i]),
    }
}

fn indexed_to_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI_RGB[usize::from(i)],
        16..=231 => {
            let idx = i - 16;
            let b = (idx % 6) * 51;
            let g = ((idx / 6) % 6) * 51;
            let r = (idx / 36) * 51;
            (r, g, b)
        }
        232..=255 => {
            let gray = (8 + u16::from(i - 232) * 10).min(255) as u8;
            (gray, gray, gray)
        }
    }
}

fn lerp_rgb(from: (u8, u8, u8), to: (f32, f32, f32), factor: f32) -> VtColor {
    let factor = factor.clamp(0.0, 1.0);
    let (r, g, b) = from;
    let nr = (r as f32 + factor * (to.0 - r as f32))
        .round()
        .clamp(0.0, 255.0) as u8;
    let ng = (g as f32 + factor * (to.1 - g as f32))
        .round()
        .clamp(0.0, 255.0) as u8;
    let nb = (b as f32 + factor * (to.2 - b as f32))
        .round()
        .clamp(0.0, 255.0) as u8;
    VtColor::Rgb(nr, ng, nb)
}

/// The terminal's 256-color palette as it reported or set it. `None` is an entry nobody told mpx
/// about, which dimming takes from the built-in palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette([Option<(u8, u8, u8)>; 256]);

impl Default for Palette {
    fn default() -> Self {
        Self([None; 256])
    }
}

impl Palette {
    pub fn get(&self, index: usize) -> Option<(u8, u8, u8)> {
        self.0.get(index).copied().flatten()
    }

    pub fn set(&mut self, index: usize, rgb: (u8, u8, u8)) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = Some(rgb);
        }
    }

    /// Puts entry `index` back to what `original` has for it.
    pub fn restore(&mut self, index: usize, original: &Palette) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = original.get(index);
        }
    }
}

/// A palette color (named, or indexed) as the terminal shows it, so dimming starts from what it
/// really displays; an RGB color, and an entry the terminal did not report, is returned as it is.
pub fn from_terminal_palette(color: VtColor, palette: &Palette) -> VtColor {
    let index = match color {
        VtColor::Indexed(i) => Some(usize::from(i)),
        other => named_index(other),
    };
    match index.and_then(|i| palette.get(i)) {
        Some((r, g, b)) => VtColor::Rgb(r, g, b),
        None => color,
    }
}

/// `color` darkened by `factor` (0.0 leaves it, 1.0 is black): each channel scales toward zero.
pub fn dim_color(color: VtColor, factor: f32) -> VtColor {
    lerp_rgb(color_to_rgb(color), (0.0, 0.0, 0.0), factor)
}

/// A cell color in an unfocused pane: dimmed by `factor`. A cell in the default color takes
/// the configured inactive color, else the terminal's own default dimmed, else stays default.
pub fn inactive_color(
    orig: Option<VtColor>,
    configured: Option<VtColor>,
    terminal_default: Option<VtColor>,
    factor: f32,
) -> Option<VtColor> {
    match orig {
        Some(c) => Some(dim_color(c, factor)),
        None => configured.or_else(|| terminal_default.map(|c| dim_color(c, factor))),
    }
}

pub fn parse_color(s: &str) -> Option<VtColor> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    match s.replace('_', "-").as_str() {
        "black" => Some(VtColor::Black),
        "red" | "dark-red" => Some(VtColor::Red),
        "green" | "dark-green" => Some(VtColor::Green),
        "yellow" | "dark-yellow" => Some(VtColor::Yellow),
        "blue" | "dark-blue" => Some(VtColor::Blue),
        "magenta" | "dark-magenta" => Some(VtColor::Magenta),
        "cyan" | "dark-cyan" => Some(VtColor::Cyan),
        "white" | "grey" | "gray" => Some(VtColor::White),
        "bright-black" | "brightblack" | "dark-grey" | "dark-gray" => Some(VtColor::BrightBlack),
        "bright-red" | "brightred" => Some(VtColor::BrightRed),
        "bright-green" | "brightgreen" => Some(VtColor::BrightGreen),
        "bright-yellow" | "brightyellow" => Some(VtColor::BrightYellow),
        "bright-blue" | "brightblue" => Some(VtColor::BrightBlue),
        "bright-magenta" | "brightmagenta" => Some(VtColor::BrightMagenta),
        "bright-cyan" | "brightcyan" => Some(VtColor::BrightCyan),
        "bright-white" | "brightwhite" => Some(VtColor::BrightWhite),
        other => {
            if let Some(num) = other
                .strip_prefix("color")
                .or_else(|| other.strip_prefix("colour"))
            {
                num.parse::<u8>().ok().map(VtColor::Indexed)
            } else if let Ok(num) = other.parse::<u8>() {
                Some(VtColor::Indexed(num))
            } else {
                None
            }
        }
    }
}

fn parse_hex_color(hex: &str) -> Option<VtColor> {
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(VtColor::Rgb(r, g, b))
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
        Some(VtColor::Rgb(r, g, b))
    } else {
        None
    }
}
