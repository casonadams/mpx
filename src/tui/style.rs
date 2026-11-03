//! tmux style strings (`fg=red,bg=#202020,bold`), for the options that colour the terminal UI and
//! for the `#[...]` directives of status formats.
use super::render::parse_color;
use crate::vt::Color as VtColor;

/// Attributes and colours of a run of text. A `None` colour is the row's own colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub fg: Option<VtColor>,
    pub bg: Option<VtColor>,
    pub bold: bool,
    pub dim: bool,
    pub underscore: bool,
    pub italics: bool,
    pub reverse: bool,
}

impl Style {
    /// `self` with what `own` sets on top: its colours where it has them, its attributes added.
    pub fn over(&self, own: &Style) -> Style {
        Style {
            fg: own.fg.or(self.fg),
            bg: own.bg.or(self.bg),
            bold: self.bold || own.bold,
            dim: self.dim || own.dim,
            underscore: self.underscore || own.underscore,
            italics: self.italics || own.italics,
            reverse: self.reverse || own.reverse,
        }
    }
}

/// A style string over the terminal's own colours, as tmux reads an option value such as
/// `status-style`. Every item must be known: `bold`, `dim`, `underscore`, `italics`, `reverse`
/// (and their `no` forms), `fg=`/`bg=` with a colour, `default` and `none`.
pub fn parse(spec: &str) -> Result<Style, String> {
    let mut style = Style::default();
    apply(&mut style, spec)?;
    Ok(style)
}

/// Applies each item of a style string to `style`, in order. An unknown item is an error and
/// leaves the items before it applied.
pub fn apply(style: &mut Style, spec: &str) -> Result<(), String> {
    for item in spec
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        apply_item(style, item)?;
    }
    Ok(())
}

/// Applies an inline `#[...]` directive of a format. Items mpx does not draw (`align=`, `list=`,
/// unknown words) are ignored, as tmux ignores what it cannot place in a format.
pub fn apply_directive(style: &mut Style, body: &str) {
    for item in body.split(',').map(str::trim) {
        // A directive item that is not a style is skipped, not an error: the format still draws.
        let _ = apply_item(style, item);
    }
}

fn apply_item(style: &mut Style, item: &str) -> Result<(), String> {
    match item {
        "default" | "none" => *style = Style::default(),
        "bold" => style.bold = true,
        "nobold" => style.bold = false,
        "dim" => style.dim = true,
        "nodim" => style.dim = false,
        "underscore" => style.underscore = true,
        "nounderscore" => style.underscore = false,
        "italics" => style.italics = true,
        "noitalics" => style.italics = false,
        "reverse" => style.reverse = true,
        "noreverse" => style.reverse = false,
        _ => match (item.strip_prefix("fg="), item.strip_prefix("bg=")) {
            (Some(colour), _) => style.fg = colour_of(colour)?,
            (None, Some(colour)) => style.bg = colour_of(colour)?,
            (None, None) => {
                return Err(format!(
                    "'{item}' is not a style item: use bold, dim, underscore, italics, reverse, \
                     their no- forms, fg=, bg=, default or none"
                ));
            }
        },
    }
    Ok(())
}

/// A colour in a style item: `default` (the row's own), a name, `colourN` or `#rrggbb`.
fn colour_of(value: &str) -> Result<Option<VtColor>, String> {
    if value == "default" {
        return Ok(None);
    }
    parse_color(value)
        .map(Some)
        .ok_or_else(|| format!("'{value}' is not a colour"))
}

/// The style of a row whose colours are `fg` and `bg` (`status-fg`, `status-bg`, a border
/// colour) with the style string `spec` applied over them.
pub fn over_colours(fg: &str, bg: &str, spec: &str) -> Result<Style, String> {
    let base = Style {
        fg: parse_color(fg),
        bg: parse_color(bg),
        ..Style::default()
    };
    Ok(base.over(&parse(spec)?))
}
