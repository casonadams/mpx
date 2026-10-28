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
    Accent,
}

impl Tone {
    fn color(self) -> Color {
        match self {
            Tone::Attention => Color::Yellow,
            Tone::Active => Color::Green,
            Tone::Plain => Color::Reset,
            Tone::Failed => Color::Red,
            Tone::Accent => Color::Cyan,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub dim: bool,
    pub bold: bool,
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
            bold: false,
        }
    }

    pub fn dim(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: Tone::Plain,
            dim: true,
            bold: false,
        }
    }

    pub fn bold(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            bold: true,
            ..Self::toned(text, tone)
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

/// Replaces a leading `home` directory with `~`.
pub fn short_path(path: &str, home: Option<&str>) -> String {
    let rest = home
        .filter(|h| !h.is_empty())
        .and_then(|h| path.strip_prefix(h));
    match rest {
        Some("") => "~".to_string(),
        Some(r) if r.starts_with(['/', '\\']) => format!("~{r}"),
        _ => path.to_string(),
    }
}

/// Last `max` lines of `text`, ignoring trailing blank lines.
pub fn tail_lines(text: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(max);
    lines[start..].iter().map(|l| l.to_string()).collect()
}

/// Non-empty `parts` joined with ` · `.
pub fn join_meta(parts: &[&str]) -> String {
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    kept.join(" · ")
}

/// Splits `area` into the first `top` rows and the rest.
pub fn split_rows(area: Rect, top: u16) -> (Rect, Rect) {
    let top = top.min(area.height);
    (
        Rect::new(area.x, area.y, area.width, top),
        Rect::new(area.x, area.y + top, area.width, area.height - top),
    )
}

/// Content rows a preview panel in `area` leaves for lines under an optional meta row.
pub fn preview_height(area: Rect, meta: bool) -> usize {
    (inner(area).height as usize).saturating_sub(usize::from(meta))
}

/// `Preview` panel: an optional meta row, then `lines`, or `empty_hint` when there are none.
pub fn draw_preview<W: Write>(
    out: &mut W,
    area: Rect,
    meta: Option<Vec<Span>>,
    lines: &[Span],
    empty_hint: Option<&str>,
) -> io::Result<()> {
    let body = inner(area);
    let offset = u16::from(meta.is_some());
    draw_panel(out, area, "Preview", "")?;
    if let Some(meta) = meta {
        put_row(out, body, 0, &meta, false)?;
    }
    if lines.is_empty() {
        if let Some(hint) = empty_hint {
            put_row(out, body, offset, &[Span::dim(hint)], false)?;
        }
        return Ok(());
    }
    for (row, line) in lines.iter().enumerate() {
        put_row(out, body, row as u16 + offset, std::slice::from_ref(line), false)?;
    }
    Ok(())
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
    if span.bold {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    queue!(out, Print(text))
}

/// Box with dim borders, a bold `title` and a dim `footer` in the edge rules.
pub fn draw_panel<W: Write>(out: &mut W, area: Rect, title: &str, footer: &str) -> io::Result<()> {
    if area.width < 2 || area.height < 2 {
        return Ok(());
    }
    let inner_w = (area.width - 2) as usize;
    let blank = " ".repeat(inner_w);
    let border = |out: &mut W, row: u16, text: &str| -> io::Result<()> {
        queue!(out, MoveTo(area.x, area.y + row))?;
        write_span(out, &Span::dim(text), false, text)
    };
    for row in 1..area.height - 1 {
        border(out, row, "│")?;
        queue!(out, Print(&blank))?;
        queue!(out, MoveTo(area.x + area.width - 1, area.y + row))?;
        write_span(out, &Span::dim("│"), false, "│")?;
    }
    let title = [Span::bold(title, Tone::Accent)];
    edge(out, area, 0, ("┌", "┐"), &title)?;
    edge(out, area, area.height - 1, ("└", "┘"), &hint_spans(footer))?;
    queue!(out, SetAttribute(Attribute::Reset))
}

/// `key label  key label` rendered with accented keys and dim labels.
fn hint_spans(hints: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    for (i, hint) in hints.split("  ").filter(|h| !h.is_empty()).enumerate() {
        let (key, label) = hint.split_once(' ').unwrap_or((hint, ""));
        spans.push(Span::dim(if i == 0 { "" } else { "  " }));
        spans.push(Span::toned(key, Tone::Accent));
        if !label.is_empty() {
            spans.push(Span::dim(format!(" {label}")));
        }
    }
    spans
}

fn edge<W: Write>(
    out: &mut W,
    area: Rect,
    row: u16,
    (left, right): (&str, &str),
    label: &[Span],
) -> io::Result<()> {
    let inner_w = (area.width - 2) as usize;
    let mut budget = inner_w.saturating_sub(4);
    let mut clipped = Vec::new();
    for span in label {
        let text: String = span.text.chars().take(budget).collect();
        budget -= text.chars().count();
        clipped.push(Span {
            text,
            ..span.clone()
        });
    }
    let len: usize = clipped.iter().map(|s| s.text.chars().count()).sum();
    let (rule_a, rule_b, used) = if len == 0 {
        ("", "", 0)
    } else {
        ("─ ", " ", len + 3)
    };
    queue!(out, MoveTo(area.x, area.y + row))?;
    let head = format!("{left}{rule_a}");
    write_span(out, &Span::dim(head.as_str()), false, &head)?;
    for span in &clipped {
        write_span(out, span, false, &span.text)?;
    }
    let tail = format!("{rule_b}{}{right}", "─".repeat(inner_w - used));
    write_span(out, &Span::dim(tail.as_str()), false, &tail)
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
