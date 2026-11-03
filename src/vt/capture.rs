//! `capture-pane`: a terminal's history and screen as text, optionally with SGR escapes.
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::grid::{Cell, CellStyle, Color, Grid};

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

fn push_sgr(out: &mut String, style: CellStyle) {
    let mut params = vec!["0".to_string()];
    for (on, code) in [
        (style.bold, "1"),
        (style.faint, "2"),
        (style.italic, "3"),
        (style.underline, "4"),
        (style.reverse, "7"),
    ] {
        if on {
            params.push(code.to_string());
        }
    }
    if let Some(fg) = style.fg {
        params.push(color_param(fg, 30));
    }
    if let Some(bg) = style.bg {
        params.push(color_param(bg, 40));
    }
    let _ = write!(out, "\x1b[{}m", params.join(";"));
}

/// SGR parameters for a color: `base` 30 for foreground, 40 for background.
fn color_param(color: Color, base: u8) -> String {
    let named = |index: u8| (base + index).to_string();
    let bright = |index: u8| (base + 60 + index).to_string();
    match color {
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::White => named(7),
        Color::BrightBlack => bright(0),
        Color::BrightRed => bright(1),
        Color::BrightGreen => bright(2),
        Color::BrightYellow => bright(3),
        Color::BrightBlue => bright(4),
        Color::BrightMagenta => bright(5),
        Color::BrightCyan => bright(6),
        Color::BrightWhite => bright(7),
        Color::Indexed(n) => format!("{};5;{n}", base + 8),
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
    }
}
