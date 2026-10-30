//! Large text for display-panes numbers and clock mode: 3x5 block glyphs, drawn at the largest
//! scale that fits, else as plain text. Pure: no terminal or daemon access.
use crate::layout::Rect;
use crate::protocol::NodeInfo;
use crate::tui::panes::PaneRect;

const GLYPH_ROWS: usize = 5;

/// Bitmap of one character, 3 cells wide and 5 high; `1` is a lit cell.
fn glyph(c: char) -> Option<[&'static str; GLYPH_ROWS]> {
    Some(match c {
        '0' => ["111", "101", "101", "101", "111"],
        '1' => ["010", "110", "010", "010", "111"],
        '2' => ["111", "001", "111", "100", "111"],
        '3' => ["111", "001", "111", "001", "111"],
        '4' => ["101", "101", "111", "001", "001"],
        '5' => ["111", "100", "111", "001", "111"],
        '6' => ["111", "100", "111", "101", "111"],
        '7' => ["111", "001", "001", "001", "001"],
        '8' => ["111", "101", "111", "101", "111"],
        '9' => ["111", "101", "111", "001", "111"],
        ':' => ["000", "010", "000", "010", "000"],
        _ => return None,
    })
}

/// Block rows of `text` with every glyph cell drawn `scale` columns wide, glyphs separated by
/// `scale` blank columns. `None` for empty text or a character without a glyph.
fn blocks(text: &str, scale: usize) -> Option<Vec<String>> {
    if text.is_empty() {
        return None;
    }
    let glyphs = text.chars().map(glyph).collect::<Option<Vec<_>>>()?;
    let mut rows = vec![String::new(); GLYPH_ROWS];
    for (i, bitmap) in glyphs.iter().enumerate() {
        if i > 0 {
            for row in &mut rows {
                row.push_str(&" ".repeat(scale));
            }
        }
        for (row, bits) in rows.iter_mut().zip(bitmap) {
            for bit in bits.chars() {
                let cell = if bit == '1' { "█" } else { " " };
                row.push_str(&cell.repeat(scale));
            }
        }
    }
    Some(rows)
}

/// The rows that draw `text` within `max_w` columns and `max_h` rows: block digits two columns per
/// cell, then one column per cell, then the plain text on one row. Empty when nothing fits. Every
/// row of the result has the same width.
pub fn label(text: &str, max_w: usize, max_h: usize) -> Vec<String> {
    for scale in [2, 1] {
        if let Some(rows) = blocks(text, scale) {
            let width = rows[0].chars().count();
            if width <= max_w && GLYPH_ROWS <= max_h {
                return rows;
            }
        }
    }
    let plain: String = text.chars().take(max_w).collect();
    if plain.is_empty() || max_h == 0 {
        Vec::new()
    } else {
        vec![plain]
    }
}

/// `height` rows of exactly `width` characters with `hour:minute` (24-hour) large in the middle,
/// as clock mode fills a pane. Empty when either size is zero.
pub fn clock_lines(hour: u8, minute: u8, width: usize, height: usize) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let block = label(&format!("{hour:02}:{minute:02}"), width, height);
    let block_width = block.first().map_or(0, |row| row.chars().count());
    let left = (width - block_width) / 2;
    let top = (height - block.len()) / 2;
    (0..height)
        .map(|y| {
            let text = y
                .checked_sub(top)
                .and_then(|i| block.get(i))
                .map_or("", String::as_str);
            format!("{:<width$}", format!("{}{text}", " ".repeat(left)))
        })
        .collect()
}

/// Where `block` (from `label`) sits centered in `area`.
pub fn label_rect(area: Rect, block: &[String]) -> Rect {
    let width = block.first().map_or(0, |row| row.chars().count()) as u16;
    let height = block.len() as u16;
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// The number each visible pane is shown and picked by, with its pane name: its position among
/// the panes of its window in node order, the numbering `list-panes` uses. A pane missing from
/// `nodes` takes its position in `panes`.
pub fn pane_numbers(panes: &[PaneRect], nodes: &[NodeInfo]) -> Vec<(usize, String)> {
    panes
        .iter()
        .enumerate()
        .map(|(position, (name, ..))| {
            let number = match nodes.iter().find(|n| n.name == *name) {
                Some(node) => nodes
                    .iter()
                    .filter(|n| n.window == node.window)
                    .position(|n| n.name == *name)
                    .unwrap_or(position),
                None => position,
            };
            (number, name.clone())
        })
        .collect()
}
