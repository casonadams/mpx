use crate::format::{Lists, expand_with};
use crate::tui::app::{App, Mode};
use crate::tui::status::local_utc_offset;
use crate::tui::statusline::{Seg, SessionInfo, parse_styled, status_lists, status_vars, strftime};

/// `fmt` expanded the way the status line expands its formats: the active pane's variables.
pub fn expand_active(app: &App, fmt: &str) -> String {
    expand_for(app, None, fmt)
}

/// `fmt` expanded for pane `target` (a name or `%id`; the active pane when `None`), with the
/// variables of that pane's window and the loop lists of the status line. A target that is not on
/// screen expands the session alone.
pub fn expand_for(app: &App, target: Option<&str>, fmt: &str) -> String {
    let wanted = target.unwrap_or(app.active_pane.as_str());
    let focused = app
        .nodes
        .iter()
        .find(|n| n.name == wanted || format!("%{}", n.id) == wanted)
        .map(|n| format_view(app, n));
    let window = focused.as_ref().map(|view| view.window);
    let tab = app
        .layout
        .tabs
        .iter()
        .find(|t| Some(t.index) == window)
        .or_else(|| app.layout.tabs.iter().find(|t| t.is_active));
    let vars = status_vars(
        &app.session,
        focused.as_ref(),
        tab,
        app.zoomed(),
        app.config.base_index,
    );
    expand_with(fmt, &vars, &loop_lists(app))
}

/// The variables status formats expand with: the active pane's and the current window's.
pub(super) fn active_vars(app: &App) -> crate::format::Vars {
    let focused = app
        .nodes
        .iter()
        .find(|n| n.name == app.active_pane)
        .map(|n| format_view(app, n));
    let tab = app.layout.tabs.iter().find(|t| t.is_active);
    status_vars(
        &app.session,
        focused.as_ref(),
        tab,
        app.zoomed(),
        app.config.base_index,
    )
}

/// The lists the loop formats iterate over: every window and the panes of the current window.
pub(super) fn loop_lists(app: &App) -> Lists {
    let panes: Vec<crate::format::PaneView> =
        app.nodes.iter().map(|n| format_view(app, n)).collect();
    let session = SessionInfo {
        name: &app.session,
        times: app.attachment.times,
        attached: app.attachment.attached,
    };
    status_lists(
        session,
        &app.layout.tabs,
        &panes,
        app.zoomed(),
        app.config.base_index,
    )
}

/// A status format expanded, styled by its `#[...]` directives, with strftime applied to its text.
pub(super) fn format_status(
    fmt: &str,
    vars: &crate::format::Vars,
    lists: &Lists,
    now: u64,
) -> Vec<Seg> {
    let offset = local_utc_offset(now);
    parse_styled(&expand_with(fmt, vars, lists))
        .into_iter()
        .map(|seg| Seg {
            text: strftime(&seg.text, now, offset),
            ..seg
        })
        .collect()
}

/// The format view of `node`, numbered the way `#{window_index}` and `#{pane_index}` show it.
fn format_view(app: &App, node: &crate::protocol::NodeInfo) -> crate::format::PaneView {
    let slot = app
        .nodes
        .iter()
        .filter(|n| n.window == node.window)
        .position(|n| n.id == node.id)
        .unwrap_or(0);
    let mut view = crate::format::pane_view(&app.session, slot, node, &app.panes, &app.layout.tabs);
    view.times = app.attachment.times;
    view.session_attached = app.attachment.attached;
    view.in_mode = app.parked_copy.contains_key(&node.name)
        || (matches!(app.mode, Mode::Copy(_)) && node.name == app.active_pane);
    view.window_base = app.config.base_index;
    view.pane_base = app.config.window.pane_base_index;
    view
}
