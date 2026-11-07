use std::io::{self, Write};

use crate::layout::Divider;
use crate::layout::geometry::Rect;
use crate::protocol::NodeInfo;
use crate::tui::app::{App, Mode};
use crate::tui::copy::{CopyState, SelectionKind};
use crate::tui::panes::PaneRect;
use crate::tui::render::{
    Compositor, DividerStyle, PaneColors, RenderPaneContext, SelectionRange, parse_color,
};
use crate::tui::status::{exit_badge, node_named};
use crate::tui::style::over_colours;
use crate::vt::Color as VtColor;
use crate::vt::VtScreen;
use crate::vt::grid::{Cell, Grid};

/// What one pane shows beyond its cells: badge, scroll, selection and dimming.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneView {
    pub badge: Option<String>,
    pub scroll_offset: usize,
    pub selection: Option<SelectionRange>,
    pub dimmed: bool,
}

/// Panes with cells to paint; zero-sized panes are skipped.
pub fn drawable(panes: &[PaneRect]) -> Vec<PaneRect> {
    panes
        .iter()
        .filter(|(_, rect, _)| rect.width > 0 && rect.height > 0)
        .cloned()
        .collect()
}

/// Decorations for one pane. `crowd` is `(visible pane count, zoomed, dim_inactive)`.
pub fn pane_view(
    mode: &Mode,
    grid: &Grid,
    nodes: &[NodeInfo],
    pane: &PaneRect,
    crowd: (usize, bool, bool),
) -> PaneView {
    let copy = match mode {
        Mode::Copy(cs) if pane.2 => Some(cs),
        _ => None,
    };
    pane_view_with(copy, grid, nodes, pane, crowd)
}

/// Decorations for one pane given the copy state it is in, if any.
fn pane_view_with(
    copy: Option<&CopyState>,
    grid: &Grid,
    nodes: &[NodeInfo],
    pane: &PaneRect,
    crowd: (usize, bool, bool),
) -> PaneView {
    let (name, _, focused) = pane;
    let (count, zoomed, dim_inactive) = crowd;
    let dimmed = is_dimmed(count, zoomed, *focused, dim_inactive);
    match copy {
        Some(cs) => PaneView {
            badge: Some(copy_label(cs, grid.history_len())),
            scroll_offset: cs.scroll_offset,
            selection: cs.selection_range(grid),
            dimmed,
        },
        None => PaneView {
            badge: exit_badge_for(nodes, name),
            scroll_offset: 0,
            selection: None,
            dimmed,
        },
    }
}

fn copy_label(cs: &CopyState, hist_len: usize) -> String {
    let label = match cs.mode {
        SelectionKind::None => "COPY",
        SelectionKind::Character => "VISUAL",
        SelectionKind::Line => "VISUAL LINE",
        SelectionKind::Rectangle => "VISUAL BLOCK",
    };
    format!(
        "[{} -{}/{} | y: yank | q: exit]",
        label, cs.scroll_offset, hist_len
    )
}

fn exit_badge_for(nodes: &[NodeInfo], name: &str) -> Option<String> {
    node_named(nodes, name).and_then(exit_badge)
}

/// Panes other than the focused one dim when several are visible and none is zoomed.
fn is_dimmed(count: usize, zoomed: bool, focused: bool, dim_inactive: bool) -> bool {
    dim_inactive && count > 1 && !zoomed && !focused
}

/// Blanks the cells of the pane area that no drawn pane or divider covers. A window laid out smaller
/// than this client leaves such cells, and what was drawn there before must not stay on screen.
/// Cells a pane or divider draws are left to them, so nothing is written twice.
pub(super) fn blank_uncovered<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let (w, h) = app.viewport;
    let top = app.rows.pane_top;
    let dividers: &[Divider] = if app.zoomed() {
        &[]
    } else {
        &app.layout.dividers
    };
    let blank = Cell {
        c: ' ',
        style: Default::default(),
    };
    for y in top..top + h {
        for x in 0..w {
            let drawn = app
                .panes
                .iter()
                .any(|(_, r, _)| r.width > 0 && r.height > 0 && r.contains(x, y))
                || dividers.iter().any(|d| divider_covers(d, x, y));
            if !drawn {
                app.canvas.put(out, x, y, blank)?;
            }
        }
    }
    Ok(())
}

/// Whether the border cell of `divider` at (`x`, `y`) is one it draws.
fn divider_covers(divider: &Divider, x: u16, y: u16) -> bool {
    if divider.is_vertical {
        x == divider.x && y >= divider.y && y - divider.y < divider.length
    } else {
        y == divider.y && x >= divider.x && x - divider.x < divider.length
    }
}

/// Panes are drawn left to right so a row that spills past its pane is repainted over by its neighbor.
/// A pane whose content scrolled is scrolled on the terminal first, so only the rows that changed
/// are sent (beside the sidebar or a split too, where the terminal has left/right margins).
pub(super) fn draw_panes<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let mut panes = drawable(&app.panes);
    panes.sort_by_key(|(_, rect, _)| (rect.x, rect.y));
    let crowd = (app.panes.len(), app.zoomed(), app.config.style.dim_inactive);
    let colors = PaneColors::new(&app.config.style, app.host.colors);
    for pane in &panes {
        if app.sync_held(&pane.0) {
            continue;
        }
        let (_, rect, _) = pane;
        let vt = app
            .vt
            .entry(pane.0.clone())
            .or_insert_with(|| VtScreen::live(rect.width as usize, rect.height as usize));
        let copy = match &app.mode {
            Mode::Copy(cs) if pane.2 => Some(cs),
            _ => app.parked_copy.get(&pane.0),
        };
        let hint = vt.grid_mut().take_scroll();
        let view = pane_view_with(copy, vt.grid(), &app.nodes, pane, crowd);
        if let Some(hint) =
            hint.filter(|h| view.scroll_offset == 0 && h.bottom < rect.height as usize)
        {
            let region = Rect::new(
                rect.x,
                rect.y + hint.top as u16,
                rect.width,
                (hint.bottom - hint.top + 1) as u16,
            );
            app.canvas.scroll(out, region, hint.lines)?;
        }
        Compositor::draw_pane_content(
            &mut app.canvas,
            out,
            RenderPaneContext::from_config(
                &colors,
                *rect,
                vt.grid(),
                view.selection.as_ref(),
                view.scroll_offset,
                view.badge.as_deref(),
                view.dimmed,
            ),
        )?;
    }
    Ok(())
}

/// The borders' colours: each pane's border colour and background, the style strings over them.
pub(super) fn divider_style(app: &App) -> DividerStyle {
    let borders = &app.config.style;
    let active = over_colours(
        &borders.border_active,
        "",
        &borders.pane_active_border_style,
    )
    .unwrap_or_default();
    let inactive =
        over_colours(&borders.border_inactive, "", &borders.pane_border_style).unwrap_or_default();
    DividerStyle {
        active_color: active.fg.unwrap_or(VtColor::BrightGreen),
        inactive_color: inactive.fg.unwrap_or(VtColor::BrightBlack),
        marked_color: parse_color(&borders.border_marked).unwrap_or(VtColor::BrightMagenta),
        border_style: borders.border_style,
        bg: app
            .config
            .style
            .dim_inactive
            .then(|| PaneColors::new(&app.config.style, app.host.colors).inactive_default_bg())
            .flatten(),
        active_bg: active.bg,
        inactive_bg: inactive.bg,
    }
}
