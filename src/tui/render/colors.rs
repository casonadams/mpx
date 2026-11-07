use crossterm::style::Color;

use crate::tui::color::{Palette, inactive_color, parse_color};
use crate::vt::Color as VtColor;

/// Colors that style the panes of one frame: the config's templates and the host terminal's own
/// default colors, which stand in for cells that set none.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PaneColors {
    pub dim_factor: f32,
    pub active_fg: Option<VtColor>,
    pub active_bg: Option<VtColor>,
    pub inactive_fg: Option<VtColor>,
    pub inactive_bg: Option<VtColor>,
    /// Default foreground and background the terminal reported at attach, when it did.
    pub host_fg: Option<VtColor>,
    pub host_bg: Option<VtColor>,
    /// The ANSI colors the terminal reported, which dimming starts from.
    pub host_palette: Palette,
}

impl PaneColors {
    pub fn new(style: &crate::config::StyleConfig, host: HostColors) -> Self {
        Self {
            dim_factor: style.dim_factor,
            active_fg: parse_color(&style.active.fg),
            active_bg: parse_color(&style.active.bg),
            inactive_fg: parse_color(&style.inactive.fg),
            inactive_bg: parse_color(&style.inactive.bg),
            host_fg: host.fg,
            host_bg: host.bg,
            host_palette: host.palette,
        }
    }

    /// What a default-colored background becomes in an inactive pane: the configured color, else
    /// the terminal's (or the active template's) default, dimmed.
    pub fn inactive_default_bg(&self) -> Option<VtColor> {
        inactive_color(
            None,
            self.inactive_bg,
            self.active_bg.or(self.host_bg),
            self.dim_factor,
        )
    }
}

/// The host terminal's default foreground and background, when it reported them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HostColors {
    pub fg: Option<VtColor>,
    pub bg: Option<VtColor>,
    pub palette: Palette,
}

impl HostColors {
    /// Follows a color sequence a pane sent to the terminal (`OSC 4`, `10`, `11`, `104`, `110`,
    /// `111`), such as a theme switcher emits, so dimming starts from the colors the terminal now
    /// shows. A reset goes back to `original`, what the terminal reported at attach. Returns
    /// whether anything changed.
    pub fn follow(&mut self, original: &HostColors, sequence: &[u8]) -> bool {
        let text = String::from_utf8_lossy(sequence);
        let Some(body) = text.strip_prefix("\x1b]") else {
            return false;
        };
        let body = body
            .strip_suffix("\x1b\\")
            .or_else(|| body.strip_suffix('\x07'))
            .unwrap_or(body);
        let before = *self;
        let mut parts = body.split(';');
        let rgb = |spec: Option<&str>| {
            spec.and_then(crate::tui::terminal::parse_color_spec)
                .map(|(r, g, b)| VtColor::Rgb(r, g, b))
        };
        match parts.next() {
            Some("4") => {
                while let (Some(index), Some(spec)) = (parts.next(), parts.next()) {
                    if let (Ok(index), Some((r, g, b))) = (
                        index.parse::<usize>(),
                        crate::tui::terminal::parse_color_spec(spec),
                    ) {
                        self.palette.set(index, (r, g, b));
                    }
                }
            }
            Some("104") => {
                let indexes: Vec<usize> = parts.filter_map(|i| i.parse().ok()).collect();
                if indexes.is_empty() {
                    self.palette = original.palette;
                }
                for index in indexes {
                    self.palette.restore(index, &original.palette);
                }
            }
            Some("10") => {
                self.fg = rgb(parts.next()).or(self.fg);
                self.bg = rgb(parts.next()).or(self.bg);
            }
            Some("11") => self.bg = rgb(parts.next()).or(self.bg),
            Some("110") => self.fg = original.fg,
            Some("111") => self.bg = original.bg,
            _ => {}
        }
        *self != before
    }

    pub fn from_report(report: &crate::tui::terminal::TerminalReport) -> Self {
        let color = |rgb: Option<(u8, u8, u8)>| rgb.map(|(r, g, b)| VtColor::Rgb(r, g, b));
        Self {
            fg: color(report.fg),
            bg: color(report.bg),
            palette: report.palette,
        }
    }
}

pub fn vt_to_crossterm(color: VtColor) -> Color {
    match color {
        VtColor::Black => Color::Black,
        VtColor::Red => Color::DarkRed,
        VtColor::Green => Color::DarkGreen,
        VtColor::Yellow => Color::DarkYellow,
        VtColor::Blue => Color::DarkBlue,
        VtColor::Magenta => Color::DarkMagenta,
        VtColor::Cyan => Color::DarkCyan,
        VtColor::White => Color::Grey,
        VtColor::BrightBlack => Color::DarkGrey,
        VtColor::BrightRed => Color::Red,
        VtColor::BrightGreen => Color::Green,
        VtColor::BrightYellow => Color::Yellow,
        VtColor::BrightBlue => Color::Blue,
        VtColor::BrightMagenta => Color::Magenta,
        VtColor::BrightCyan => Color::Cyan,
        VtColor::BrightWhite => Color::White,
        VtColor::Indexed(n) => Color::AnsiValue(n),
        VtColor::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}
