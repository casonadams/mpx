//! Draws one frame from app state. Wrapped in a DEC 2026 synchronized update so it lands atomically.
use std::io::{self, Write};

use super::app::{App, Mode};
use super::copy::{CopyState, SelectionKind};
use super::panes::PaneRect;
use super::render::{
    Compositor, DividerStyle, RenderPaneContext, SelectionRange, StatusBar, hud_rect, parse_color,
};
use super::status::exit_badge;
use crate::layout::geometry::Rect;
use crate::protocol::NodeInfo;
use crate::vt::Color as VtColor;
use crate::vt::VtScreen;
use crate::vt::grid::Grid;

/// What one pane shows beyond its cells: badge, scroll, selection and dimming.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneView {
    pub badge: Option<String>,
    pub scroll_offset: usize,
    pub selection: Option<SelectionRange>,
    pub dimmed: bool,
}

pub fn draw<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    Compositor::begin_sync(out)?;
    let (screen_w, layout_h) = app.viewport;
    let screen_h = layout_h + 1;
    let hud = shortcut_popup(app, screen_w, screen_h);
    let covered = match &app.mode {
        Mode::Overlay(_) => Some(overlay_area(screen_w, screen_h)),
        _ => hud,
    };
    app.canvas.begin(screen_w, layout_h, covered);
    draw_panes(app, out)?;
    if app.zoomed.is_none() && !app.layout.dividers.is_empty() {
        let style = DividerStyle {
            active_color: parse_color(&app.config.style.border_active)
                .unwrap_or(VtColor::BrightGreen),
            inactive_color: parse_color(&app.config.style.border_inactive)
                .unwrap_or(VtColor::BrightBlack),
            border_style: app.config.style.border_style,
        };
        Compositor::draw_dividers(
            &mut app.canvas,
            out,
            &app.layout.dividers,
            app.active_rect,
            style,
        )?;
    }
    app.canvas.finish(out)?;
    draw_status_row(app, out)?;
    if hud.is_some() {
        Compositor::draw_shortcut_hud(out, screen_w, screen_h)?;
    }
    if let Mode::Overlay(overlay) = &app.mode {
        overlay.render(out, overlay_area(screen_w, screen_h))?;
    }
    place_cursor(app, out)?;
    Compositor::end_sync(out)?;
    out.flush()
}

/// The shortcut popup's area, once due and when it fits on screen.
fn shortcut_popup(app: &App, screen_w: u16, screen_h: u16) -> Option<Rect> {
    if !app.hud_visible() {
        return None;
    }
    hud_rect(screen_w, screen_h)
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
    let (name, _, focused) = pane;
    let (count, zoomed, dim_inactive) = crowd;
    let dimmed = is_dimmed(count, zoomed, *focused, dim_inactive);
    match mode {
        Mode::Copy(cs) if *focused => PaneView {
            badge: Some(copy_label(cs, grid.history_len())),
            scroll_offset: cs.scroll_offset,
            selection: cs.selection_range(grid),
            dimmed,
        },
        _ => PaneView {
            badge: exit_badge_for(nodes, name),
            scroll_offset: 0,
            selection: None,
            dimmed,
        },
    }
}

/// Panes are drawn left to right so a row that spills past its pane is repainted over by its neighbor.
fn draw_panes<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let mut panes = drawable(&app.panes);
    panes.sort_by_key(|(_, rect, _)| (rect.x, rect.y));
    let crowd = (
        app.panes.len(),
        app.zoomed.is_some(),
        app.config.style.dim_inactive,
    );
    let dim_factor = app.config.style.dim_factor;
    let dim_content_bg = app.config.style.dim_content_bg;
    for pane in &panes {
        let (_, rect, _) = pane;
        let vt = app
            .vt
            .entry(pane.0.clone())
            .or_insert_with(|| VtScreen::new(rect.width as usize, rect.height as usize));
        let view = pane_view(&app.mode, vt.grid(), &app.nodes, pane, crowd);
        Compositor::draw_pane_content(
            &mut app.canvas,
            out,
            RenderPaneContext {
                rect: *rect,
                grid: vt.grid(),
                selection: view.selection.as_ref(),
                scroll_offset: view.scroll_offset,
                badge: view.badge.as_deref(),
                dimmed: view.dimmed,
                dim_factor,
                dim_content_bg,
            },
        )?;
    }
    Ok(())
}

/// Bottom row: the command line while typing or confirming, else a live message, else the status bar.
/// Sent only when it differs from the row already on screen.
fn draw_status_row<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let mut row = Vec::new();
    status_row(app, &mut row)?;
    if app.canvas.status_changed(&row) {
        out.write_all(&row)?;
    }
    Ok(())
}

fn status_row(app: &App, out: &mut Vec<u8>) -> io::Result<()> {
    let (width, y) = app.viewport;
    let bg = parse_color(&app.config.style.status_bg);
    let fg = parse_color(&app.config.style.status_fg);
    match &app.mode {
        Mode::Prompt(prompt) => {
            Compositor::draw_command_line(out, &format!(":{}", prompt.text()), y, width)
        }
        Mode::Confirm(question, _) => Compositor::draw_command_line(out, question, y, width),
        Mode::Copy(cs) if cs.search_prompt.is_some() => {
            let search = cs.search_prompt.as_ref().unwrap();
            let prefix = search.direction.prompt_char();
            Compositor::draw_command_line(out, &format!("{prefix}{}", search.prompt.text()), y, width)
        }
        _ => match app.live_message() {
            Some(text) => Compositor::draw_command_line(out, text, y, width),
            None => Compositor::draw_status_bar(
                out,
                &StatusBar {
                    session: &app.session,
                    tabs: &app.layout.tabs,
                    panes: &app.panes,
                    prefix_active: matches!(app.mode, Mode::Prefix),
                    waiting_agents: app.watch.badge,
                    width,
                    y,
                    bg,
                    fg,
                    zoomed: app.zoomed.is_some(),
                    copy_pane: match &app.mode {
                        Mode::Copy(_) => Some(app.active_pane.as_str()),
                        _ => None,
                    },
                },
            ),
        },
    }
}

fn place_cursor<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    let (width, y) = app.viewport;
    match &app.mode {
        Mode::Prompt(prompt) => Compositor::position_cursor(
            out,
            Rect::new(0, y, width, 1),
            1 + prompt.cursor(),
            0,
            true,
        ),
        Mode::Copy(cs) if cs.search_prompt.is_some() => {
            let search = cs.search_prompt.as_ref().unwrap();
            Compositor::position_cursor(
                out,
                Rect::new(0, y, width, 1),
                1 + search.prompt.cursor(),
                0,
                true,
            )
        }
        Mode::Overlay(_) | Mode::Confirm(..) => {
            Compositor::position_cursor(out, Rect::new(0, 0, 1, 1), 0, 0, false)
        }
        Mode::Copy(cs) => place_pane_cursor(app, out, Some(cs)),
        _ => place_pane_cursor(app, out, None),
    }
}

fn place_pane_cursor<W: Write>(app: &App, out: &mut W, copy: Option<&CopyState>) -> io::Result<()> {
    let (Some(rect), Some(vt)) = (app.active_rect, app.active_vt()) else {
        return Ok(());
    };
    let (x, y, visible) = match copy {
        Some(cs) => (cs.cursor_x, cs.cursor_y, true),
        None => {
            let cursor = vt.grid().cursor;
            (cursor.x, cursor.y, cursor.visible)
        }
    };
    Compositor::position_cursor(out, rect, x, y, visible)
}

/// Centered overlay area covering 90% of the screen.
fn overlay_area(screen_w: u16, screen_h: u16) -> Rect {
    let w = screen_w * 9 / 10;
    let h = screen_h * 9 / 10;
    Rect::new((screen_w - w) / 2, (screen_h - h) / 2, w, h)
}

fn copy_label(cs: &CopyState, hist_len: usize) -> String {
    let label = match cs.mode {
        SelectionKind::None => "COPY",
        SelectionKind::Character => "VISUAL",
        SelectionKind::Line => "VISUAL LINE",
    };
    format!(
        "[{} -{}/{} | y: yank | q: exit]",
        label, cs.scroll_offset, hist_len
    )
}

fn exit_badge_for(nodes: &[NodeInfo], name: &str) -> Option<String> {
    nodes.iter().find(|n| n.name == name).and_then(exit_badge)
}

/// Panes other than the focused one dim when several are visible and none is zoomed.
fn is_dimmed(count: usize, zoomed: bool, focused: bool, dim_inactive: bool) -> bool {
    dim_inactive && count > 1 && !zoomed && !focused
}
