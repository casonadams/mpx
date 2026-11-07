use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
};

use crate::layout::geometry::Rect;
use crate::tui::app::{App, Mode};
use crate::tui::labels::{clock_lines, label, label_rect, pane_numbers};
use crate::tui::render::hud_rect;

/// The shortcut popup's area, once due and when it fits on screen.
pub(super) fn shortcut_popup(app: &App, screen_w: u16, screen_h: u16) -> Option<Rect> {
    if !app.hud_visible() {
        return None;
    }
    hud_rect(screen_w, screen_h)
}

/// The cells an open mode or the shortcut HUD covers: the canvas leaves them to the overlay.
pub(super) fn covered_by(app: &App, hud: Option<Rect>, screen_w: u16, screen_h: u16) -> Vec<Rect> {
    match &app.mode {
        Mode::Overlay(overlay) => vec![overlay.footprint(overlay_area(screen_w, screen_h))],
        Mode::Clock { pane } => app
            .panes
            .iter()
            .filter(|(name, ..)| name == pane)
            .map(|(_, rect, _)| *rect)
            .collect(),
        Mode::Panes { .. } => number_boxes(app).into_iter().map(|(at, ..)| at).collect(),
        Mode::Popup(popup) => vec![popup.rect],
        Mode::Menu(menu) => vec![menu.rect],
        _ => hud.into_iter().collect(),
    }
}

/// The mode's own drawing on top of the panes: pane numbers, the clock, a popup or a menu.
pub(super) fn draw_mode_overlay<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    match &app.mode {
        Mode::Panes { .. } => draw_pane_numbers(app, out),
        Mode::Clock { pane } => draw_clock(app, pane, out),
        Mode::Popup(popup) => popup.render(out),
        Mode::Menu(menu) => menu.render(out),
        _ => Ok(()),
    }
}

/// Centered overlay area covering 90% of the screen.
pub(super) fn overlay_area(screen_w: u16, screen_h: u16) -> Rect {
    let w = screen_w * 9 / 10;
    let h = screen_h * 9 / 10;
    Rect::new((screen_w - w) / 2, (screen_h - h) / 2, w, h)
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
        draw_rows(out, at.x, at.y, &rows)?;
    }
    queue!(out, ResetColor)
}

/// clock-mode: the clock in `pane`, filling its area.
fn draw_clock<W: Write>(app: &App, pane: &str, out: &mut W) -> io::Result<()> {
    let Some((_, area, _)) = app.panes.iter().find(|(name, ..)| name.as_str() == pane) else {
        return Ok(());
    };
    let (hour, minute) = app.chrome.clock.hour_minute();
    let rows = clock_lines(hour, minute, area.width as usize, area.height as usize);
    queue!(out, SetForegroundColor(Color::Cyan))?;
    draw_rows(out, area.x, area.y, &rows)?;
    queue!(out, ResetColor)
}

/// Prints `rows` one below the other, the first at (`x`, `y`).
fn draw_rows<W: Write>(out: &mut W, x: u16, y: u16, rows: &[String]) -> io::Result<()> {
    for (dy, row) in rows.iter().enumerate() {
        queue!(out, MoveTo(x, y + dy as u16), Print(row))?;
    }
    Ok(())
}
