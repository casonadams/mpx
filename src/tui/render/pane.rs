use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use super::Compositor;
use super::colors::PaneColors;
use super::selection::SelectionRange;
use crate::layout::geometry::Rect;
use crate::tui::canvas::Canvas;
use crate::tui::color::{Palette, from_terminal_palette, inactive_color};
use crate::vt::grid::Grid;
use crate::vt::{Cell, CellStyle, Color as VtColor};

pub struct RenderPaneContext<'a> {
    pub rect: Rect,
    pub grid: &'a Grid,
    pub selection: Option<&'a SelectionRange>,
    pub scroll_offset: usize,
    pub badge: Option<&'a str>,
    pub dimmed: bool,
    pub dim_factor: f32,
    pub active_fg: Option<VtColor>,
    pub active_bg: Option<VtColor>,
    pub inactive_fg: Option<VtColor>,
    pub inactive_bg: Option<VtColor>,
    /// What a default-colored cell is: the configured active color, else the host terminal's.
    pub default_fg: Option<VtColor>,
    pub default_bg: Option<VtColor>,
    /// The terminal's own ANSI colors, when it reported them; dimming starts from these.
    pub palette: Palette,
}

impl<'a> RenderPaneContext<'a> {
    pub fn from_config(
        colors: &PaneColors,
        rect: Rect,
        grid: &'a Grid,
        selection: Option<&'a SelectionRange>,
        scroll_offset: usize,
        badge: Option<&'a str>,
        dimmed: bool,
    ) -> Self {
        Self {
            rect,
            grid,
            selection,
            scroll_offset,
            badge,
            dimmed,
            dim_factor: colors.dim_factor,
            active_fg: colors.active_fg,
            active_bg: colors.active_bg,
            inactive_fg: colors.inactive_fg,
            inactive_bg: colors.inactive_bg,
            default_fg: colors.active_fg.or(colors.host_fg),
            default_bg: colors.active_bg.or(colors.host_bg),
            palette: colors.host_palette,
        }
    }
}

impl Compositor {
    /// Draws a pane's cells (with selection, scroll offset and badge) into its rect on `canvas`.
    pub fn draw_pane_content<W: Write>(
        canvas: &mut Canvas,
        w: &mut W,
        ctx: RenderPaneContext<'_>,
    ) -> io::Result<()> {
        let badge_chars = ctx.badge.map(|b| b.chars().collect::<Vec<char>>());
        let badge_info = badge_chars.as_ref().and_then(|chars| {
            if (ctx.rect.width as usize) > chars.len() + 1 {
                let start_col = (ctx.rect.width as usize) - chars.len() - 1;
                Some((start_col, chars.as_slice()))
            } else {
                None
            }
        });
        let mut cells = Vec::with_capacity(ctx.rect.width as usize);
        for row in 0..ctx.rect.height as usize {
            let abs_row = ctx.grid.abs_line(row, ctx.scroll_offset);
            cells.clear();
            cells.extend(
                (0..ctx.rect.width as usize)
                    .map(|col| cell_content(col, row, abs_row, &ctx, badge_info)),
            );
            canvas.put_row(w, ctx.rect.x, ctx.rect.y + row as u16, &cells)?;
        }
        Ok(())
    }

    pub fn draw_copy_mode_badge<W: Write>(
        w: &mut W,
        rect: Rect,
        mode_label: &str,
        scroll_offset: usize,
        total_history: usize,
    ) -> io::Result<()> {
        let text = format!(
            " [{} | -{}/{} lines | y: yank | q: exit] ",
            mode_label, scroll_offset, total_history
        );
        let badge_x = (rect.x + rect.width).saturating_sub(text.len() as u16 + 1);
        if badge_x >= rect.x {
            queue!(
                w,
                MoveTo(badge_x, rect.y),
                SetBackgroundColor(Color::Rgb {
                    r: 255,
                    g: 140,
                    b: 0
                }),
                SetForegroundColor(Color::Black),
                Print(text),
                ResetColor
            )?;
        }
        Ok(())
    }
}

fn cell_content(
    col: usize,
    row: usize,
    abs_row: usize,
    ctx: &RenderPaneContext<'_>,
    badge_info: Option<(usize, &[char])>,
) -> Cell {
    if row == 0
        && let Some((b_start, b_chars)) = badge_info
        && col >= b_start
        && col < b_start + b_chars.len()
    {
        return highlighted(b_chars[col - b_start]);
    }
    let cell = ctx
        .grid
        .get_cell_scrolled(col, row, ctx.scroll_offset)
        .unwrap_or_default();
    if ctx.selection.is_some_and(|s| s.contains_abs(col, abs_row)) {
        highlighted(cell.c)
    } else if ctx.dimmed {
        Cell {
            c: cell.c,
            style: CellStyle {
                fg: inactive_color(
                    cell.style
                        .fg
                        .map(|c| from_terminal_palette(c, &ctx.palette)),
                    ctx.inactive_fg,
                    ctx.default_fg,
                    ctx.dim_factor,
                ),
                bg: inactive_color(
                    cell.style
                        .bg
                        .map(|c| from_terminal_palette(c, &ctx.palette)),
                    ctx.inactive_bg,
                    ctx.default_bg,
                    ctx.dim_factor,
                ),
                bold: false,
                ..cell.style
            },
        }
    } else {
        apply_active_style(cell, ctx.active_fg, ctx.active_bg)
    }
}

pub fn apply_active_style(
    cell: Cell,
    active_fg: Option<VtColor>,
    active_bg: Option<VtColor>,
) -> Cell {
    let fg = cell.style.fg.or(active_fg);
    let bg = cell.style.bg.or(active_bg);
    if fg == cell.style.fg && bg == cell.style.bg {
        cell
    } else {
        Cell {
            c: cell.c,
            style: CellStyle {
                fg,
                bg,
                ..cell.style
            },
        }
    }
}

/// Black on orange, for badges and copy-mode selection.
fn highlighted(c: char) -> Cell {
    Cell {
        c,
        style: CellStyle {
            fg: Some(VtColor::Black),
            bg: Some(VtColor::Rgb(255, 140, 0)),
            ..CellStyle::default()
        },
    }
}
