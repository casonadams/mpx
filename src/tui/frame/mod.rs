//! Draws one frame from app state. Wrapped in a DEC 2026 synchronized update so it lands atomically.
mod bar;
mod expand;
mod overlay;
mod pane;

use std::io::{self, Write};

use crate::layout::geometry::Rect;
use crate::tui::app::{App, Mode};
use crate::tui::copy::CopyState;
use crate::tui::render::Compositor;

use bar::{chrome_text, draw_status_extra, draw_status_row};
use overlay::{covered_by, draw_mode_overlay, overlay_area, shortcut_popup};
use pane::{blank_uncovered, divider_style, draw_panes};

pub use expand::{expand_active, expand_for};
pub use pane::{PaneView, drawable, pane_view};

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
    let mut covered = covered_by(app, hud, screen_w, screen_h);
    if app.rows.status.is_none() && chrome_text(app).is_some() {
        covered.push(Rect::new(0, app.rows.prompt, screen_w, 1));
    }
    app.canvas.begin(screen_w, screen_h, covered);
    blank_uncovered(app, out)?;
    draw_panes(app, out)?;
    if !app.zoomed() && !app.layout.dividers.is_empty() {
        let style = divider_style(app);
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
    draw_status_extra(app, out)?;
    draw_mode_overlay(app, out)?;
    if hud.is_some() {
        Compositor::draw_shortcut_hud(out, screen_w, screen_h)?;
    }
    if let Mode::Overlay(overlay) = &app.mode {
        overlay.render(out, overlay_area(screen_w, screen_h))?;
    }
    Ok(())
}

/// Places and shows (or hides) the terminal cursor where the frame wants it.
pub fn place_cursor<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    let width = app.viewport.0;
    let y = app.rows.prompt;
    match &app.mode {
        Mode::Prompt(prompt) => Compositor::position_cursor(
            out,
            Rect::new(0, y, width, 1),
            prompt.line.label().chars().count() + prompt.line.cursor(),
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
        Mode::Popup(popup) => match popup.cursor() {
            Some((x, y)) => Compositor::position_cursor(out, Rect::new(x, y, 1, 1), 0, 0, true),
            None => Compositor::position_cursor(out, Rect::new(0, 0, 1, 1), 0, 0, false),
        },
        Mode::Menu(_) => Compositor::position_cursor(out, Rect::new(0, 0, 1, 1), 0, 0, false),
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
