//! Sizes and positions of popups and menus, as `display-popup` and `display-menu` take them.

/// A size along one axis: a number of cells or a percentage of the screen. Unsized, a popup or
/// menu takes 80% of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    Cells(u16),
    Percent(u16),
}

impl Default for Extent {
    fn default() -> Self {
        Self::Percent(80)
    }
}

/// Where a popup or menu starts along one axis: a cell offset from the start of the screen, a
/// percentage of it, centred, or flush with the far edge (`-x R` / `-y R`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Offset {
    Cells(u16),
    Percent(u16),
    #[default]
    Centre,
    Far,
}

impl Extent {
    /// `40` cells or `80%`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.strip_suffix('%') {
            Some(percent) => Ok(Self::Percent(percent_value(percent, raw)?)),
            None => raw
                .parse()
                .map(Self::Cells)
                .map_err(|_| format!("not a size: {raw}")),
        }
    }

    /// The size on a screen `screen` cells long, never less than one cell.
    pub fn cells(self, screen: u16) -> u16 {
        match self {
            Self::Cells(n) => n.max(1),
            Self::Percent(p) => (u32::from(screen) * u32::from(p) / 100).max(1) as u16,
        }
    }
}

impl Offset {
    /// `C` (centred), `R` (far edge), `12` cells or `50%`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "C" => Ok(Self::Centre),
            "R" => Ok(Self::Far),
            _ => match raw.strip_suffix('%') {
                Some(percent) => Ok(Self::Percent(percent_value(percent, raw)?)),
                None => raw
                    .parse()
                    .map(Self::Cells)
                    .map_err(|_| format!("not a position: {raw} (give cells, N%, C or R)")),
            },
        }
    }

    /// Where a box `extent` cells long starts on a screen `screen` cells long, kept on the screen.
    pub fn start(self, screen: u16, extent: u16) -> u16 {
        let room = screen.saturating_sub(extent);
        match self {
            Self::Cells(n) => n.min(room),
            Self::Percent(p) => (u32::from(screen) * u32::from(p) / 100).min(u32::from(room)) as u16,
            Self::Centre => room / 2,
            Self::Far => room,
        }
    }
}

fn percent_value(digits: &str, raw: &str) -> Result<u16, String> {
    match digits.parse::<u16>() {
        Ok(p) if p <= 100 => Ok(p),
        _ => Err(format!("not a percentage: {raw}")),
    }
}
