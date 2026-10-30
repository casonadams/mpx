//! Draws one frame from app state. Wrapped in a DEC 2026 synchronized update so it lands atomically.
use std::io::{self, Write};

use super::app::{App, Mode};
use super::copy::{CopyState, SelectionKind};
use super::panes::PaneRect;
use super::render::{
    Compositor, DividerStyle, RenderPaneContext, SelectionRange, StatusBar, hud_rect, parse_color,
};
use super::status::{exit_badge, node_named};
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
    let (mut body, mut cursor) = (Vec::new(), Vec::new());
    draw_body(app, &mut body)?;
    place_cursor(app, &mut cursor)?;
    send(out, &body, &cursor)
}

/// Writes a frame's body and cursor placement in one synchronized update, so it lands at once.
pub fn send<W: Write>(out: &mut W, body: &[u8], cursor: &[u8]) -> io::Result<()> {
    Compositor::begin_sync(out)?;
    out.write_all(body)?;
    out.write_all(cursor)?;
    Compositor::end_sync(out)?;
    out.flush()
}

/// Everything a frame changes on screen but the cursor: nothing at all when no cell, chrome or
/// overlay changed.
pub fn draw_body<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let (screen_w, layout_h) = app.viewport;
    let screen_h = layout_h + 1;
    let hud = shortcut_popup(app, screen_w, screen_h);
    let covered: Vec<Rect> = match &app.mode {
        Mode::Overlay(_) => vec![overlay_area(screen_w, screen_h)],
        _ => hud.into_iter().collect(),
    };
    app.canvas.begin(screen_w, layout_h, covered);
    draw_panes(app, out)?;
    if app.zoomed.is_none() && !app.layout.dividers.is_empty() {
        let style = DividerStyle {
            active_color: parse_color(&app.config.style.border_active)
                .unwrap_or(VtColor::BrightGreen),
            inactive_color: parse_color(&app.config.style.border_inactive)
                .unwrap_or(VtColor::BrightBlack),
            marked_color: parse_color(&app.config.style.border_marked)
                .unwrap_or(VtColor::BrightMagenta),
            border_style: app.config.style.border_style,
        };
        Compositor::draw_dividers(
            &mut app.canvas,
            out,
            &app.layout.dividers,
            app.active_rect,
            app.marked_rect,
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
    Ok(())
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


/// Panes are drawn left to right so a row that spills past its pane is repainted over by its neighbor.
/// A pane whose content scrolled is scrolled on the terminal first, so only the rows that changed
/// are sent (beside the sidebar or a split too, where the terminal has left/right margins).
fn draw_panes<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let mut panes = drawable(&app.panes);
    panes.sort_by_key(|(_, rect, _)| (rect.x, rect.y));
    let crowd = (
        app.panes.len(),
        app.zoomed.is_some(),
        app.config.style.dim_inactive,
    );
    for pane in &panes {
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
                &app.config.style,
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
            Compositor::draw_command_line(
                out,
                &format!("{prefix}{}", search.prompt.text()),
                y,
                width,
            )
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
                    width,
                    y,
                    bg,
                    fg,
                    zoomed: app.zoomed.is_some(),
                    copy_pane: match &app.mode {
                        Mode::Copy(_) => Some(app.active_pane.as_str()),
                        _ if app.parked_copy.contains_key(&app.active_pane) => {
                            Some(app.active_pane.as_str())
                        }
                        _ => None,
                    },
                    marked_pane: app.layout.marked_pane_global().map(str::to_string),
                },
            ),
        },
    }
}

/// Places and shows (or hides) the terminal cursor where the frame wants it.
pub fn place_cursor<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
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
        _ => place_pane_cursor(app, out, app.parked_copy.get(&app.active_pane)),
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
    node_named(nodes, name).and_then(exit_badge)
}

/// Panes other than the focused one dim when several are visible and none is zoomed.
fn is_dimmed(count: usize, zoomed: bool, focused: bool, dim_inactive: bool) -> bool {
    dim_inactive && count > 1 && !zoomed && !focused
}
