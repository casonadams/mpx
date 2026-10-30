//! Draws one frame from app state. Wrapped in a DEC 2026 synchronized update so it lands atomically.
use std::io::{self, Write};

use super::app::{App, Mode};
use super::copy::{CopyState, SelectionKind};
use super::labels::{clock_lines, label, label_rect, pane_numbers};
use super::panes::PaneRect;
use super::render::{
    Compositor, DividerStyle, PaneColors, RenderPaneContext, SelectionRange, StatusBar, hud_rect,
    parse_color,
};
use super::render::{draw_status_segments, get_current_time, status_right};
use super::status::{exit_badge, node_named};
use super::status::{local_utc_offset, pane_items, tab_items};
use super::statusline::{
    DEFAULT_WINDOW_FORMAT, Item, Line, Seg, Target, compose, parse_styled, status_vars, strftime,
    truncate,
};
use crate::layout::geometry::Rect;
use crate::protocol::NodeInfo;
use crate::tmux_shim::expand;
use crate::vt::Color as VtColor;
use crate::vt::VtScreen;
use crate::vt::grid::Grid;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
};

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
    let screen_w = app.viewport.0;
    let screen_h = app.rows.screen_h;
    let hud = shortcut_popup(app, screen_w, screen_h);
    let mut covered: Vec<Rect> = match &app.mode {
        Mode::Overlay(_) => vec![overlay_area(screen_w, screen_h)],
        Mode::Clock { pane } => app
            .panes
            .iter()
            .filter(|(name, ..)| name == pane)
            .map(|(_, rect, _)| *rect)
            .collect(),
        Mode::Panes { .. } => number_boxes(app).into_iter().map(|(at, ..)| at).collect(),
        _ => hud.into_iter().collect(),
    };
    if app.rows.status.is_none() && chrome_text(app).is_some() {
        covered.push(Rect::new(0, app.rows.prompt, screen_w, 1));
    }
    app.canvas.begin(screen_w, screen_h, covered);
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
    match &app.mode {
        Mode::Panes { .. } => draw_pane_numbers(app, out)?,
        Mode::Clock { pane } => draw_clock(app, pane, out)?,
        _ => {}
    }
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
    let colors = PaneColors::new(&app.config.style, app.host_colors);
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

/// Message row or status line: the message row when a prompt, confirmation, copy-mode search or
/// live message is up, else the status line (nothing when the status line is hidden).
fn status_row(app: &mut App, out: &mut Vec<u8>) -> io::Result<()> {
    let width = app.viewport.0;
    if let Some(text) = chrome_text(app) {
        return Compositor::draw_command_line(out, &text, app.rows.prompt, width);
    }
    let Some(y) = app.rows.status else {
        return Ok(());
    };
    let bg = parse_color(&app.config.style.status_bg);
    let fg = parse_color(&app.config.style.status_fg);
    let line = status_line(app, width as usize);
    draw_status_segments(out, &line.segs, y, bg, fg)?;
    app.status_targets = line.targets;
    Ok(())
}

/// The text of the message row when it replaces the status line.
fn chrome_text(app: &App) -> Option<String> {
    let search = match &app.mode {
        Mode::Copy(cs) => cs
            .search_prompt
            .as_ref()
            .map(|s| format!("{}{}", s.direction.prompt_char(), s.prompt.text())),
        _ => None,
    };
    search.or_else(|| match &app.mode {
        Mode::Prompt(prompt) => Some(format!(":{}", prompt.text())),
        Mode::Confirm(question, _) => Some(question.clone()),
        _ => app.live_message().map(str::to_string),
    })
}

/// The status line for a row `width` cells wide, laid out from the configuration and the state.
fn status_line(app: &App, width: usize) -> Line {
    let config = &app.config;
    let zoomed = app.zoomed.is_some();
    let base = config.base_index;
    let tab = app.layout.tabs.iter().find(|t| t.is_active);
    let focused = app
        .nodes
        .iter()
        .find(|n| n.name == app.active_pane)
        .map(|n| shim_view(app, n));
    let vars = status_vars(&app.session, focused.as_ref(), tab, zoomed, base);
    let now = crate::node::now_ms();
    let left = match &config.status_left {
        None => vec![Seg::plain(format!("[{}] ", app.session))],
        Some(fmt) => truncate(format_status(fmt, &vars, now), config.status_left_length),
    };
    let right = match &config.status_right {
        None => vec![Seg::plain(built_in_right(app))],
        Some(fmt) => truncate(format_status(fmt, &vars, now), config.status_right_length),
    };
    let (items, separator) = window_items(app, zoomed);
    compose(width, left, items, &separator, right, config.status_justify)
}

/// A status format expanded, styled by its `#[...]` directives, with strftime applied to its text.
fn format_status(fmt: &str, vars: &crate::tmux_shim::Vars, now: u64) -> Vec<Seg> {
    let offset = local_utc_offset(now);
    parse_styled(&expand(fmt, vars))
        .into_iter()
        .map(|seg| Seg {
            text: strftime(&seg.text, now, offset),
            ..seg
        })
        .collect()
}

/// The window list: clickable tabs (mpx's own, or built from `window-status-format`), or pane names
/// when the session has no windows. Returns the items and the separator between them.
fn window_items(app: &App, zoomed: bool) -> (Vec<Item>, String) {
    let config = &app.config;
    let base = config.base_index;
    if app.layout.tabs.is_empty() {
        let marked = app.layout.marked_pane_global();
        return (pane_items(&app.panes, zoomed, marked), String::new());
    }
    let custom = config.window_status_format.is_some()
        || config.window_status_current_format.is_some()
        || config.window_status_separator.is_some();
    if !custom {
        return (tab_items(&app.layout.tabs, zoomed, base), String::new());
    }
    let items = app
        .layout
        .tabs
        .iter()
        .map(|tab| {
            let fmt = match (tab.is_active, &config.window_status_current_format) {
                (true, Some(current)) => current.as_str(),
                _ => config
                    .window_status_format
                    .as_deref()
                    .unwrap_or(DEFAULT_WINDOW_FORMAT),
            };
            let focused = app
                .nodes
                .iter()
                .find(|n| n.window == Some(tab.index))
                .map(|n| shim_view(app, n));
            let vars = status_vars(&app.session, focused.as_ref(), Some(tab), zoomed, base);
            Item {
                segs: parse_styled(&expand(fmt, &vars)),
                target: Some(Target::Window(tab.index)),
            }
        })
        .collect();
    let separator = config
        .window_status_separator
        .clone()
        .unwrap_or_else(|| " ".to_string());
    (items, separator)
}

/// The shim's view of `node`, numbered the way `#{window_index}` and `#{pane_index}` show it.
fn shim_view(app: &App, node: &crate::protocol::NodeInfo) -> crate::tmux_shim::PaneView {
    let slot = app
        .nodes
        .iter()
        .filter(|n| n.window == node.window)
        .position(|n| n.id == node.id)
        .unwrap_or(0);
    let mut view =
        crate::tmux_shim::pane_view(&app.session, slot, node, &app.panes, &app.layout.tabs);
    view.window_base = app.config.base_index;
    view.pane_base = app.config.pane_base_index;
    view
}

/// mpx's built-in right side: the copy-mode badge, the zoom flag and the clock.
fn built_in_right(app: &App) -> String {
    let copy_pane = match &app.mode {
        Mode::Copy(_) => Some(app.active_pane.as_str()),
        _ if app.parked_copy.contains_key(&app.active_pane) => Some(app.active_pane.as_str()),
        _ => None,
    };
    let bar = StatusBar {
        session: &app.session,
        tabs: &app.layout.tabs,
        panes: &app.panes,
        prefix_active: false,
        width: 0,
        y: 0,
        bg: None,
        fg: None,
        zoomed: app.zoomed.is_some(),
        copy_pane,
        marked_pane: None,
    };
    status_right(&bar, &get_current_time())
}

/// Places and shows (or hides) the terminal cursor where the frame wants it.
pub fn place_cursor<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    let width = app.viewport.0;
    let y = app.rows.prompt;
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

/// display-panes: each visible pane's number as large text centered on the pane, with the rectangle
/// it covers, the rows to draw and whether the pane is the active one.
fn number_boxes(app: &App) -> Vec<(Rect, Vec<String>, bool)> {
    pane_numbers(&app.panes, &app.nodes)
        .into_iter()
        .filter_map(|(number, name)| {
            let (_, area, focused) = app.panes.iter().find(|(n, ..)| *n == name)?;
            let text = number.to_string();
            let rows = label(&text, area.width as usize, area.height as usize);
            Some((label_rect(*area, &rows), rows, *focused))
        })
        .collect()
}

fn draw_pane_numbers<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    for (at, rows, focused) in number_boxes(app) {
        let color = if focused { Color::Green } else { Color::Yellow };
        queue!(out, SetForegroundColor(color))?;
        for (y, row) in rows.iter().enumerate() {
            queue!(out, MoveTo(at.x, at.y + y as u16), Print(row))?;
        }
    }
    queue!(out, ResetColor)
}

/// clock-mode: the clock in `pane`, filling its area.
fn draw_clock<W: Write>(app: &App, pane: &str, out: &mut W) -> io::Result<()> {
    let Some((_, area, _)) = app.panes.iter().find(|(name, ..)| name.as_str() == pane) else {
        return Ok(());
    };
    let (hour, minute) = app.clock.hour_minute();
    let rows = clock_lines(hour, minute, area.width as usize, area.height as usize);
    queue!(out, SetForegroundColor(Color::Cyan))?;
    for (y, row) in rows.iter().enumerate() {
        queue!(out, MoveTo(area.x, area.y + y as u16), Print(row))?;
    }
    queue!(out, ResetColor)
}
