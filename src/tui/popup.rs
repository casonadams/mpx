//! Popups (tmux `display-popup`): a program drawn in a floating pane over the panes. The daemon runs
//! the program as an unplaced pane; every attached terminal shows it, sized and placed by the popup's
//! spec, and sends it the keys while it is open.
use std::io::{self, Write};

use crossterm::event::KeyEvent;

use super::app::{App, Flow, Mode};
use super::canvas::Canvas;
use super::panes::sync_dims;
use super::render::{Compositor, PaneColors, RenderPaneContext, parse_color};
use crate::config::BorderStyle;
use crate::keys::encode_key;
use crate::layout::geometry::Rect;
use crate::protocol::{PopupAnchor, PopupSize, PopupSpec};
use crate::vt::{Cell, CellStyle, Color as VtColor};

/// The popup on screen: the daemon's pane for it, and whether its program has exited.
#[derive(Debug, Clone)]
pub struct PopupView {
    pub name: String,
    pub spec: PopupSpec,
    /// The program exited: the popup stays, showing its output, until a key closes it.
    pub finished: bool,
}

/// Outer rectangle of a popup (border included) placed in `screen`. `pane` is the active pane's
/// rectangle, which the `P` positions refer to.
pub fn popup_rect(spec: &PopupSpec, screen: Rect, pane: Option<Rect>) -> Rect {
    let width = extent(spec.width, screen.width).clamp(1, screen.width.max(1));
    let height = extent(spec.height, screen.height).clamp(1, screen.height.max(1));
    let x = place(spec.x, screen.x, screen.width, width, pane.map(|p| p.x));
    let y = place(spec.y, screen.y, screen.height, height, pane.map(|p| p.y));
    Rect::new(x, y, width, height)
}

fn extent(size: PopupSize, of: u16) -> u16 {
    match size {
        PopupSize::Cells(cells) => cells,
        PopupSize::Percent(percent) => (u32::from(of) * u32::from(percent) / 100) as u16,
    }
}

fn place(anchor: PopupAnchor, origin: u16, span: u16, size: u16, pane: Option<u16>) -> u16 {
    let room = span.saturating_sub(size);
    let offset = match anchor {
        PopupAnchor::Start => 0,
        PopupAnchor::Centre => room / 2,
        PopupAnchor::End => room,
        PopupAnchor::Cells(cells) => cells,
        PopupAnchor::Pane => pane.map_or(0, |p| p.saturating_sub(origin)),
    };
    origin + offset.min(room)
}

/// The rectangle a program's screen fills: the popup's outer rectangle less its border.
pub fn inner_rect(outer: Rect, border: bool) -> Rect {
    let inset = u16::from(border);
    Rect::new(
        outer.x + inset,
        outer.y + inset,
        outer.width.saturating_sub(2 * inset).max(1),
        outer.height.saturating_sub(2 * inset).max(1),
    )
}

/// The popup's outer rectangle on the current screen, when a popup is open.
pub fn area(app: &App) -> Option<Rect> {
    let Mode::Popup(view) = &app.mode else {
        return None;
    };
    let (width, height) = app.viewport;
    let screen = Rect::new(0, app.rows.pane_top, width, height);
    Some(popup_rect(&view.spec, screen, app.active_rect))
}

/// Glyphs of a border: horizontal, vertical, then the top-left, top-right, bottom-left and
/// bottom-right corners.
fn glyphs(lines: BorderStyle) -> [char; 6] {
    match lines {
        BorderStyle::Single => ['─', '│', '┌', '┐', '└', '┘'],
        BorderStyle::Rounded => ['─', '│', '╭', '╮', '╰', '╯'],
        BorderStyle::Double => ['═', '║', '╔', '╗', '╚', '╝'],
        BorderStyle::Ascii => ['-', '|', '+', '+', '+', '+'],
    }
}

fn draw_border<W: Write>(
    canvas: &mut Canvas,
    out: &mut W,
    outer: Rect,
    lines: BorderStyle,
    color: VtColor,
    footer: &str,
) -> io::Result<()> {
    if outer.width < 2 || outer.height < 2 {
        return Ok(());
    }
    let [h, v, tl, tr, bl, br] = glyphs(lines);
    let style = CellStyle {
        fg: Some(color),
        ..CellStyle::default()
    };
    let cell = |c: char| Cell { c, style };
    let (left, right) = (outer.x, outer.x + outer.width - 1);
    let (top, bottom) = (outer.y, outer.y + outer.height - 1);
    for x in left + 1..right {
        canvas.put(out, x, top, cell(h))?;
        canvas.put(out, x, bottom, cell(h))?;
    }
    for y in top + 1..bottom {
        canvas.put(out, left, y, cell(v))?;
        canvas.put(out, right, y, cell(v))?;
    }
    canvas.put(out, left, top, cell(tl))?;
    canvas.put(out, right, top, cell(tr))?;
    canvas.put(out, left, bottom, cell(bl))?;
    canvas.put(out, right, bottom, cell(br))?;
    for (i, c) in footer.chars().enumerate() {
        let x = left + 2 + i as u16;
        if x >= right {
            break;
        }
        canvas.put(out, x, bottom, cell(c))?;
    }
    Ok(())
}

/// Draws the open popup over the frame: its border, then its program's screen. It has its own
/// canvas, so it is repainted only where it changed, and cells it covered are repainted by the
/// panes once it closes.
pub fn draw<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let Some(outer) = area(app) else {
        return Ok(());
    };
    let Mode::Popup(view) = &app.mode else {
        return Ok(());
    };
    let (name, spec, finished) = (view.name.clone(), view.spec.clone(), view.finished);
    let inner = inner_rect(outer, spec.border);
    let (screen_w, screen_h) = (app.viewport.0, app.rows.screen_h);
    app.popup_canvas
        .begin(screen_w, screen_h, std::iter::empty::<Rect>());
    if spec.border {
        let lines = spec.lines.unwrap_or(app.config.style.border_style);
        let color = spec
            .border_color
            .as_deref()
            .and_then(parse_color)
            .or_else(|| parse_color(&app.config.style.border_active))
            .unwrap_or(VtColor::BrightGreen);
        let footer = if finished {
            " exited: press a key to close "
        } else {
            ""
        };
        draw_border(&mut app.popup_canvas, out, outer, lines, color, footer)?;
    }
    let colors = PaneColors::new(&app.config.style, app.host_colors);
    if let Some(vt) = app.vt.get(&name) {
        let ctx = RenderPaneContext::from_config(&colors, inner, vt.grid(), None, 0, None, false);
        Compositor::draw_pane_content(&mut app.popup_canvas, out, ctx)?;
    }
    app.popup_canvas.finish(out)
}

/// Places the terminal cursor on the popup's program's cursor, hidden once the program has exited.
pub fn place_cursor<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    let (Some(outer), Mode::Popup(view)) = (area(app), &app.mode) else {
        return Ok(());
    };
    let inner = inner_rect(outer, view.spec.border);
    let (x, y, visible) = match app.vt.get(&view.name) {
        Some(vt) => {
            let cursor = vt.grid().cursor;
            (cursor.x, cursor.y, cursor.visible && !view.finished)
        }
        None => (0, 0, false),
    };
    Compositor::position_cursor(out, inner, x, y, visible)
}

impl App {
    /// Whether `name` is the open popup's pane.
    pub fn is_popup(&self, name: &str) -> bool {
        matches!(&self.mode, Mode::Popup(view) if view.name == name)
    }

    /// A popup opened: it is shown, and its program is sized before any of its output is read.
    pub async fn open_popup(&mut self, name: String, spec: PopupSpec) {
        self.mode = Mode::Popup(PopupView {
            name,
            spec,
            finished: false,
        });
        self.popup_canvas.invalidate();
        self.sync_popup().await;
    }

    /// The program of the open popup exited: the popup stays, showing its output, until a key
    /// closes it.
    pub fn mark_popup_finished(&mut self, name: &str) {
        if let Mode::Popup(view) = &mut self.mode
            && view.name == name
        {
            view.finished = true;
        }
    }

    /// The popup closed: its screen is dropped, and the panes it covered are repainted.
    pub fn close_popup_screen(&mut self, name: &str) {
        self.vt.remove(name);
        self.expected.remove(name);
        self.pane_dims.remove(name);
        if self.is_popup(name) {
            self.mode = Mode::Normal;
        }
        self.popup_canvas.invalidate();
    }

    /// Sizes the open popup's program to its inner rectangle at the current layout, reloading its
    /// screen when the size changed.
    pub async fn sync_popup(&mut self) {
        let Some(outer) = area(self) else {
            return;
        };
        let Mode::Popup(view) = &self.mode else {
            return;
        };
        let name = view.name.clone();
        let inner = inner_rect(outer, view.spec.border);
        sync_dims(
            &mut self.client,
            &[(name, inner, true)],
            &mut self.pane_dims,
            &mut self.vt,
            &mut self.expected,
        )
        .await;
    }

    /// Keys go to the popup's program. A finished popup closes on any key.
    pub async fn popup_key(&mut self, view: PopupView, key: KeyEvent) -> Flow {
        if view.finished {
            let _ = self.client.close_popup().await;
            return Flow::Continue;
        }
        let modes = self.modes_of(&view.name);
        let bytes = encode_key(&key, &modes);
        if !bytes.is_empty() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let _ = self.client.send_input(&view.name, &text).await;
        }
        self.mode = Mode::Popup(view);
        Flow::Continue
    }
}
