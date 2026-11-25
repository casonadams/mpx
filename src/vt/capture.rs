//! `capture-pane`: a terminal's history and screen as text, optionally with SGR escapes.
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::grid::{Cell, CellStyle, Color, Grid, Underline};
use super::parser::VtScreen;

/// A line named by `capture-pane -S` or `-E`, in tmux's numbering: 0 is the first visible line,
/// negative numbers count lines back into history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLine {
    /// The start of the history (`-S -`).
    First,
    /// The last visible line (`-E -`).
    Last,
    Line(i64),
}

/// Which lines a capture takes and how they are rendered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSpec {
    /// First line; the first visible line when `None`.
    #[serde(default)]
    pub start: Option<CaptureLine>,
    /// Last line; the last visible line when `None`.
    #[serde(default)]
    pub end: Option<CaptureLine>,
    /// Keep SGR escape sequences for colors and attributes (`-e`).
    #[serde(default)]
    pub escapes: bool,
    /// Join wrapped lines (`-J`). A wrapped line is kept to its full width, since all of its cells
    /// were written; other lines lose their trailing blanks as without `-J`.
    #[serde(default)]
    pub join: bool,
}

/// Text of the lines `spec` selects, history before the screen. Each line ends with a newline,
/// except a wrapped line that is joined onto the next one.
pub fn capture_text(grid: &Grid, spec: &CaptureSpec) -> String {
    let history = grid.history_len();
    let last = history + grid.rows - 1;
    let at = |line: CaptureLine| match line {
        CaptureLine::First => 0,
        CaptureLine::Last => last,
        CaptureLine::Line(n) => (history as i64).saturating_add(n).clamp(0, last as i64) as usize,
    };
    let start = spec.start.map_or(history, at);
    let end = spec.end.map_or(last, at);
    let mut out = String::new();
    for abs in start..=end {
        let Some(cells) = grid.line_cells(abs) else {
            break;
        };
        let joined = spec.join && grid.line_wrapped(abs);
        let used = if joined {
            cells.len()
        } else {
            cells.iter().rposition(|c| c.c != ' ').map_or(0, |i| i + 1)
        };
        push_line(&mut out, &cells[..used], spec.escapes);
        if !joined {
            out.push('\n');
        }
    }
    out
}

/// What a restored pane shows before its new process writes: `log`, output a pane retained at
/// `size` (cols, rows), replayed with up to `scrollback` lines of history and turned into its text
/// and colors only. Lines end in CRLF and wrapped lines are joined, so they reflow at any width;
/// the modes, queries and alternate screen of the program that wrote `log` are left behind.
pub fn restored_scrollback(log: &[u8], size: (usize, usize), scrollback: usize) -> Vec<u8> {
    let mut screen = VtScreen::new(size.0, size.1);
    screen.grid_mut().set_history_limit(scrollback);
    screen.feed(log);
    let grid = screen.grid();
    let end = match (grid.alternate_screen(), grid.history_len()) {
        (false, _) => CaptureLine::Last,
        (true, 0) => return Vec::new(),
        (true, _) => CaptureLine::Line(-1),
    };
    let spec = CaptureSpec {
        start: Some(CaptureLine::First),
        end: Some(end),
        escapes: true,
        join: true,
    };
    let text = capture_text(grid, &spec);
    let text = text.trim_end_matches('\n');
    if text.is_empty() {
        return Vec::new();
    }
    format!("{}\r\n", text.replace('\n', "\r\n")).into_bytes()
}

fn push_line(out: &mut String, cells: &[Cell], escapes: bool) {
    if !escapes {
        out.extend(cells.iter().map(|cell| cell.c));
        return;
    }
    let mut current = CellStyle::default();
    for cell in cells {
        if cell.style != current {
            push_sgr(out, cell.style);
            current = cell.style;
        }
        out.push(cell.c);
    }
    if current != CellStyle::default() {
        out.push_str("\x1b[0m");
    }
}

pub(super) fn push_sgr(out: &mut String, style: CellStyle) {
    out.push_str("\x1b[0");
    for (on, code) in [
        (style.bold, "1"),
        (style.faint, "2"),
        (style.italic, "3"),
        (style.reverse, "7"),
    ] {
        if on {
            let _ = write!(out, ";{code}");
        }
    }
    if let Some(code) = underline_code(style.underline) {
        let _ = write!(out, ";{code}");
    }
    if let Some(fg) = style.fg {
        push_color(out, fg, 30);
    }
    if let Some(bg) = style.bg {
        push_color(out, bg, 40);
    }
    if let Some(color) = style.underline_color {
        push_underline_color(out, color);
    }
    out.push('m');
}

/// The SGR parameter for an underline style: `4` for a single line, `4:n` for the others.
fn underline_code(underline: Underline) -> Option<&'static str> {
    match underline {
        Underline::None => None,
        Underline::Single => Some("4"),
        Underline::Double => Some("4:2"),
        Underline::Curly => Some("4:3"),
        Underline::Dotted => Some("4:4"),
        Underline::Dashed => Some("4:5"),
    }
}

/// Appends the SGR 58 parameters that set an underline colour: `58;5;n` for palette entries and
/// `58;2;r;g;b` for truecolour.
fn push_underline_color(out: &mut String, color: Color) {
    match color {
        Color::Indexed(_) | Color::Rgb(..) => push_extended(out, color, 58),
        named => push_extended(out, Color::Indexed(slot_of(named)), 58),
    }
}

/// Appends the SGR parameter for a color: `base` 30 for foreground, 40 for background.
fn push_color(out: &mut String, color: Color, base: u8) {
    match color {
        Color::Indexed(_) | Color::Rgb(..) => push_extended(out, color, base + 8),
        named => {
            let slot = slot_of(named);
            let code = if slot < 8 {
                base + slot
            } else {
                base + 60 + slot - 8
            };
            let _ = write!(out, ";{code}");
        }
    }
}

/// Appends `38`, `48` or `58` followed by a palette index (`5;n`) or truecolour (`2;r;g;b`).
fn push_extended(out: &mut String, color: Color, code: u8) {
    let _ = match color {
        Color::Indexed(n) => write!(out, ";{code};5;{n}"),
        Color::Rgb(r, g, b) => write!(out, ";{code};2;{r};{g};{b}"),
        named => unreachable!("{named:?} has no extended form"),
    };
}

/// The slot of a colour known to be named: callers route only named colours here.
fn slot_of(named: Color) -> u8 {
    named
        .named_slot()
        .unwrap_or_else(|| unreachable!("{named:?} is a named colour"))
}
