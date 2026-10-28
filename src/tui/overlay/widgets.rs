//! Drawing and text helpers shared by the overlay views. Everything stays inside the rect it is given.
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{Attribute, Color, Print, SetAttribute, SetForegroundColor};

use crate::layout::Rect;
use crate::protocol::AgentState;
use crate::tui::overlay::nav::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Attention,
    Active,
    Plain,
    Failed,
}

impl Tone {
    fn color(self) -> Color {
        match self {
            Tone::Attention => Color::Yellow,
            Tone::Active => Color::Green,
            Tone::Plain => Color::Reset,
            Tone::Failed => Color::Red,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub dim: bool,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::toned(text, Tone::Plain)
    }

    pub fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
            dim: false,
        }
    }

    pub fn dim(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: Tone::Plain,
            dim: true,
        }
    }
}

/// Glyph and tone for an agent state.
pub fn state_mark(state: AgentState) -> (char, Tone) {
    match state {
        AgentState::NeedsInput => ('◆', Tone::Attention),
        AgentState::Working => ('●', Tone::Active),
        AgentState::Idle => ('○', Tone::Plain),
        AgentState::Exited => ('✕', Tone::Failed),
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Compact age: `12s`, `3m`, `2h`, `5d`.
pub fn age_label(elapsed_ms: u64) -> String {
    let secs = elapsed_ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// Truncates with `…` or pads with spaces to exactly `width` characters.
pub fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if width == 0 {
        String::new()
    } else if count > width {
        let mut cut: String = text.chars().take(width - 1).collect();
        cut.push('…');
        cut
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}

/// Interior of a bordered box.
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

/// Rows of `area`'s interior left for list items when a prompt line may take the last row.
pub fn body(area: Rect, prompt: bool) -> Rect {
    let interior = inner(area);
    let height = if prompt {
        interior.height.saturating_sub(1)
    } else {
        interior.height
    };
    Rect::new(interior.x, interior.y, interior.width, height)
}

fn rule(label: &str, width: usize, fill: char) -> String {
    let text = if label.is_empty() {
        String::new()
    } else {
        format!("{fill} {label} ")
    };
    let text: String = text.chars().take(width).collect();
    let pad = width.saturating_sub(text.chars().count());
    format!("{text}{}", fill.to_string().repeat(pad))
}

/// Single-line box with `title` in the top border and `footer` in the bottom border.
pub fn draw_box<W: Write>(out: &mut W, area: Rect, title: &str, footer: &str) -> io::Result<()> {
    if area.width < 2 || area.height < 2 {
        return Ok(());
    }
    let inner_w = (area.width - 2) as usize;
    let blank = " ".repeat(inner_w);
    for row in 0..area.height {
        let (left, mid, right) = if row == 0 {
            ("┌", rule(title, inner_w, '─'), "┐")
        } else if row == area.height - 1 {
            ("└", rule(footer, inner_w, '─'), "┘")
        } else {
            ("│", blank.clone(), "│")
        };
        queue!(
            out,
            MoveTo(area.x, area.y + row),
            Print(left),
            Print(mid),
            Print(right)
        )?;
    }
    Ok(())
}

fn write_span<W: Write>(out: &mut W, span: &Span, selected: bool, text: &str) -> io::Result<()> {
    queue!(
        out,
        SetAttribute(Attribute::Reset),
        SetForegroundColor(span.tone.color())
    )?;
    if selected {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    }
    if span.dim {
        queue!(out, SetAttribute(Attribute::Dim))?;
    }
    queue!(out, Print(text))
}

/// Writes one line of styled spans into row `row` of `area`, clipped and padded to the full width.
pub fn put_row<W: Write>(
    out: &mut W,
    area: Rect,
    row: u16,
    spans: &[Span],
    selected: bool,
) -> io::Result<()> {
    if row >= area.height || area.width == 0 {
        return Ok(());
    }
    let width = area.width as usize;
    let mut used = 0;
    queue!(out, MoveTo(area.x, area.y + row))?;
    for span in spans {
        let text: String = span.text.chars().take(width - used).collect();
        used += text.chars().count();
        write_span(out, span, selected, &text)?;
    }
    let pad = Span::plain(" ".repeat(width - used));
    write_span(out, &pad, selected, &pad.text)
}

/// Draws `items` inside `area`, scrolling so that `selected` stays visible; the selected item is reversed.
pub fn draw_items<W: Write>(
    out: &mut W,
    area: Rect,
    items: &[Vec<Span>],
    selected: usize,
    list: &ListState,
) -> io::Result<()> {
    for (row, idx) in list
        .window(selected, items.len(), area.height as usize)
        .enumerate()
    {
        put_row(out, area, row as u16, &items[idx], idx == selected)?;
    }
    Ok(())
}

/// Last row of `area`'s interior, used for filter and input prompts.
pub fn draw_prompt<W: Write>(out: &mut W, area: Rect, text: &str) -> io::Result<()> {
    let interior = inner(area);
    put_row(
        out,
        interior,
        interior.height.saturating_sub(1),
        &[Span::plain(text)],
        false,
    )
}
