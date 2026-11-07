use std::io::{self, Write};

use super::Compositor;
use crate::config::BorderStyle;
use crate::layout::{Divider, geometry::Rect};
use crate::tui::canvas::Canvas;
use crate::vt::{Cell, CellStyle, Color as VtColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerStyle {
    pub active_color: VtColor,
    pub inactive_color: VtColor,
    pub marked_color: VtColor,
    pub border_style: BorderStyle,
    /// Background of the border cells: the inactive panes' default background, so a border
    /// beside an inactive pane does not show the terminal's own.
    pub bg: Option<VtColor>,
    /// Background from `pane-border-style` for inactive panes' borders, else `bg`.
    pub inactive_bg: Option<VtColor>,
    /// Background from `pane-active-border-style` for the active pane's borders, else `bg`.
    pub active_bg: Option<VtColor>,
}

impl Default for DividerStyle {
    fn default() -> Self {
        Self {
            active_color: VtColor::BrightGreen,
            inactive_color: VtColor::BrightBlack,
            marked_color: VtColor::BrightMagenta,
            border_style: BorderStyle::Single,
            bg: None,
            inactive_bg: None,
            active_bg: None,
        }
    }
}

fn divider_paint(
    x: u16,
    y: u16,
    active: Option<Rect>,
    marked: Option<Rect>,
    style: DividerStyle,
) -> (VtColor, Option<VtColor>) {
    let touches = |r: Rect| {
        let is_left = x + 1 == r.x && y + 1 >= r.y && y <= r.y + r.height;
        let is_right = x == r.x + r.width && y + 1 >= r.y && y <= r.y + r.height;
        let is_top = y + 1 == r.y && x + 1 >= r.x && x <= r.x + r.width;
        let is_bottom = y == r.y + r.height && x + 1 >= r.x && x <= r.x + r.width;
        is_left || is_right || is_top || is_bottom
    };
    if let Some(m) = marked
        && touches(m)
    {
        return (style.marked_color, None);
    }
    if let Some(a) = active
        && touches(a)
    {
        return (style.active_color, style.active_bg);
    }
    (style.inactive_color, style.inactive_bg)
}

fn divider_cell(
    div: &Divider,
    (x, y): (u16, u16),
    all: &[Divider],
    active: Option<Rect>,
    marked: Option<Rect>,
    style: DividerStyle,
) -> Cell {
    let (fg, own_bg) = divider_paint(x, y, active, marked, style);
    Cell {
        c: get_divider_char(div, x, y, all, style.border_style),
        style: CellStyle {
            fg: Some(fg),
            bg: own_bg.or(style.bg),
            ..CellStyle::default()
        },
    }
}
fn vertical_char(style: BorderStyle) -> char {
    match style {
        BorderStyle::Double => '║',
        BorderStyle::Ascii => '|',
        BorderStyle::Single | BorderStyle::Rounded => '│',
    }
}

fn horizontal_char(style: BorderStyle) -> char {
    match style {
        BorderStyle::Double => '═',
        BorderStyle::Ascii => '-',
        BorderStyle::Single | BorderStyle::Rounded => '─',
    }
}

fn junction_char(up: bool, down: bool, left: bool, right: bool, style: BorderStyle) -> char {
    match style {
        BorderStyle::Ascii => match (up, down, left, right) {
            (true, true, false, false) => '|',
            (false, false, true, true) => '-',
            _ => '+',
        },
        BorderStyle::Double => match (up, down, left, right) {
            (true, true, true, true) => '╬',
            (true, true, true, false) => '╣',
            (true, true, false, true) => '╠',
            (false, true, true, true) => '╦',
            (true, false, true, true) => '╩',
            (false, true, false, true) => '╔',
            (false, true, true, false) => '╗',
            (true, false, false, true) => '╚',
            (true, false, true, false) => '╝',
            (true, true, false, false) => '║',
            (false, false, true, true) => '═',
            _ => '╬',
        },
        BorderStyle::Rounded => match (up, down, left, right) {
            (false, true, false, true) => '╭',
            (false, true, true, false) => '╮',
            (true, false, false, true) => '╰',
            (true, false, true, false) => '╯',
            (true, true, true, true) => '┼',
            (true, true, true, false) => '┤',
            (true, true, false, true) => '├',
            (false, true, true, true) => '┬',
            (true, false, true, true) => '┴',
            (true, true, false, false) => '│',
            (false, false, true, true) => '─',
            _ => '┼',
        },
        BorderStyle::Single => match (up, down, left, right) {
            (true, true, true, true) => '┼',
            (true, true, true, false) => '┤',
            (true, true, false, true) => '├',
            (false, true, true, true) => '┬',
            (true, false, true, true) => '┴',
            (false, true, false, true) => '┌',
            (false, true, true, false) => '┐',
            (true, false, false, true) => '└',
            (true, false, true, false) => '┘',
            (true, true, false, false) => '│',
            (false, false, true, true) => '─',
            _ => '┼',
        },
    }
}

fn get_divider_char(div: &Divider, x: u16, y: u16, all: &[Divider], style: BorderStyle) -> char {
    let has_v = all
        .iter()
        .any(|d| d.is_vertical && d.x == x && (y + 1 >= d.y && y <= d.y + d.length));
    let has_h = all
        .iter()
        .any(|d| !d.is_vertical && d.y == y && (x + 1 >= d.x && x <= d.x + d.length));

    if has_v && has_h {
        let up = all.iter().any(|d| d.is_vertical && d.x == x && d.y < y);
        let down = all
            .iter()
            .any(|d| d.is_vertical && d.x == x && (d.y == y + 1 || d.y + d.length > y));
        let left = all.iter().any(|d| !d.is_vertical && d.y == y && d.x < x);
        let right = all
            .iter()
            .any(|d| !d.is_vertical && d.y == y && (d.x == x + 1 || d.x + d.length > x));
        junction_char(up, down, left, right, style)
    } else if div.is_vertical {
        vertical_char(style)
    } else {
        horizontal_char(style)
    }
}

/// Cells a divider covers, top to bottom or left to right.
fn divider_cells(div: &Divider) -> impl Iterator<Item = (u16, u16)> + '_ {
    (0..div.length).map(move |i| {
        if div.is_vertical {
            (div.x, div.y + i)
        } else {
            (div.x + i, div.y)
        }
    })
}

impl Compositor {
    /// Draws tmux-style dividers on `canvas`, highlighting the active pane's edges according to `style`.
    pub fn draw_dividers<W: Write>(
        canvas: &mut Canvas,
        w: &mut W,
        dividers: &[Divider],
        active_rect: Option<Rect>,
        marked_rect: Option<Rect>,
        style: DividerStyle,
    ) -> io::Result<()> {
        for div in dividers {
            for pos in divider_cells(div) {
                let cell = divider_cell(div, pos, dividers, active_rect, marked_rect, style);
                canvas.put(w, pos.0, pos.1, cell)?;
            }
        }
        Ok(())
    }
}
