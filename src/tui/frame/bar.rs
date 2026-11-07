use std::io::{self, Write};

use super::expand::{active_vars, format_status, loop_lists};
use crate::config::Justify;
use crate::format::{Lists, Vars, expand_with};
use crate::tui::app::{App, Mode};
use crate::tui::render::{Compositor, draw_status_segments, parse_color};
use crate::tui::status::pane_items;
use crate::tui::statusline::{Item, Line, Seg, Target, compose, parse_styled, truncate};
use crate::tui::style::{self, Style, over_colours};

/// Bottom row: the command line while typing or confirming, else a live message, else the status bar.
/// Sent only when it differs from the row already on screen.
pub(super) fn draw_status_row<W: Write>(app: &mut App, out: &mut W) -> io::Result<()> {
    let mut row = Vec::new();
    status_row(app, &mut row)?;
    if app.canvas.status_changed(&row) {
        out.write_all(&row)?;
    }
    Ok(())
}

/// The status rows below the first, each drawing its `status-format` with the active pane's
/// variables. Drawn on every frame, so a message or overlay that covered a row never leaves it stale.
pub(super) fn draw_status_extra<W: Write>(app: &App, out: &mut W) -> io::Result<()> {
    let first = app.rows.status.unwrap_or(0);
    let width = usize::from(app.viewport.0);
    let bg = parse_color(&app.config.style.status_bg);
    let fg = parse_color(&app.config.style.status_fg);
    let vars = active_vars(app);
    let lists = loop_lists(app);
    let now = crate::node::now_ms();
    for y in app.rows.extra_status_rows() {
        let fmt = app
            .config
            .status_format
            .get(usize::from(y - first))
            .map_or("", String::as_str);
        let line = format_row(fmt, &vars, &lists, now, width);
        draw_status_segments(out, &line.segs, y, bg, fg)?;
    }
    Ok(())
}

/// Message row or status line: the message row when a prompt, confirmation, copy-mode search or
/// live message is up, else the status line (nothing when the status line is hidden).
fn status_row(app: &mut App, out: &mut Vec<u8>) -> io::Result<()> {
    let width = app.viewport.0;
    if let Some(text) = chrome_text(app) {
        let style = chrome_style(app);
        return Compositor::draw_command_line(out, &text, app.rows.prompt, width, &style);
    }
    let Some(y) = app.rows.status else {
        return Ok(());
    };
    let config = &app.config.style;
    let row = over_colours(&config.status_fg, &config.status_bg, &config.status_style)
        .unwrap_or_default();
    let bg = parse_color(&config.status_bg);
    let fg = parse_color(&config.status_fg);
    let mut line = match app
        .config
        .status_format
        .first()
        .filter(|fmt| !fmt.is_empty())
    {
        Some(fmt) => format_row(
            fmt,
            &active_vars(app),
            &loop_lists(app),
            crate::node::now_ms(),
            width as usize,
        ),
        None => status_line(app, width as usize),
    };
    for seg in &mut line.segs {
        seg.style = row.over(&seg.style);
    }
    draw_status_segments(out, &line.segs, y, bg, fg)?;
    app.status_hits.targets = line.targets;
    app.status_hits.zones = (line.left_end, line.right_start);
    Ok(())
}

/// `fmt` expanded and laid out as one row `width` cells wide, with no window list.
fn format_row(fmt: &str, vars: &Vars, lists: &Lists, now: u64, width: usize) -> Line {
    let cells = truncate(format_status(fmt, vars, lists, now), width);
    compose(width, cells, Vec::new(), "", Vec::new(), Justify::Left)
}

/// The style of the message row: the command style while a prompt, a confirmation or a search
/// is up, else the message style.
pub(super) fn chrome_style(app: &App) -> Style {
    let spec = match &app.mode {
        Mode::Prompt(_) | Mode::Confirm(..) => &app.config.style.message_command_style,
        Mode::Copy(cs) if cs.search_prompt.is_some() => &app.config.style.message_command_style,
        _ => &app.config.style.message_style,
    };
    style::parse(spec).unwrap_or_default()
}

/// The text of the message row when it replaces the status line.
pub(super) fn chrome_text(app: &App) -> Option<String> {
    let search = match &app.mode {
        Mode::Copy(cs) => cs
            .search_prompt
            .as_ref()
            .map(|s| format!("{}{}", s.direction.prompt_char(), s.prompt.text())),
        _ => None,
    };
    search.or_else(|| match &app.mode {
        Mode::Prompt(prompt) => Some(format!("{}{}", prompt.line.label(), prompt.line.text())),
        Mode::Confirm(question, _) => Some(question.clone()),
        _ => app.live_message().map(str::to_string),
    })
}

/// The status line for a row `width` cells wide, laid out from the configuration and the state.
fn status_line(app: &App, width: usize) -> Line {
    let config = &app.config;
    let zoomed = app.zoomed();
    let vars = active_vars(app);
    let lists = loop_lists(app);
    let now = crate::node::now_ms();
    let left = truncate(
        format_status(&config.status_left, &vars, &lists, now),
        config.status_left_length,
    );
    let right = truncate(
        format_status(&config.status_right, &vars, &lists, now),
        config.status_right_length,
    );
    let (items, separator) = window_items(app, zoomed, &lists);
    compose(width, left, items, &separator, right, config.status_justify)
}

/// The window list: clickable tabs (mpx's own, or built from `window-status-format`), or pane names
/// when the session has no windows. Returns the items and the separator between them.
fn window_items(app: &App, zoomed: bool, lists: &Lists) -> (Vec<Item>, String) {
    let config = &app.config;
    if app.layout.tabs.is_empty() {
        let marked = app.layout.marked_pane_global();
        return (pane_items(&app.panes, zoomed, marked), String::new());
    }
    let items = app
        .layout
        .tabs
        .iter()
        .zip(&lists.windows)
        .map(|(tab, vars)| {
            let fmt = if tab.is_active {
                &config.window_status_current_format
            } else {
                &config.window_status_format
            };
            let tab_look = tab_style(&config.style, tab);
            Item {
                segs: parse_styled(&expand_with(fmt, vars, lists))
                    .into_iter()
                    .map(|seg| Seg {
                        style: tab_look.over(&seg.style),
                        ..seg
                    })
                    .collect(),
                target: Some(Target::Window(tab.index)),
            }
        })
        .collect();
    (items, config.window_status_separator.clone())
}

/// The style of a window tab: `window-status-style`, then those of the tab's last, current,
/// activity and bell state, each laid over the one before.
fn tab_style(config: &crate::config::StyleConfig, tab: &crate::layout::WindowTab) -> Style {
    let layers = [
        (true, &config.window_status_style),
        (tab.is_last, &config.window_status_last_style),
        (tab.is_active, &config.window_status_current_style),
        (tab.has_activity, &config.window_status_activity_style),
        (tab.has_bell, &config.window_status_bell_style),
    ];
    layers
        .into_iter()
        .filter(|(on, _)| *on)
        .fold(Style::default(), |style, (_, spec)| {
            style.over(&style::parse(spec).unwrap_or_default())
        })
}
