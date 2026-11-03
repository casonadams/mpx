//! Mouse: click focuses a pane; dragging from the click starts a selection; release yanks; wheel scrolls;
//! dragging a split border resizes the panes beside it.
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::app::{App, Flow, Mode};
use super::copy::CopyState;
use super::panes::{BorderGrab, border_at, border_move};
use crate::layout::geometry::Rect;
use super::command;
use super::mouse_menu::MouseMenu;

/// Cell position inside `rect`, clamped to its edges.
fn local_point(rect: Rect, col: u16, row: u16) -> (usize, usize) {
    let x = (col.saturating_sub(rect.x) as usize).min(rect.width.saturating_sub(1) as usize);
    let y = (row.saturating_sub(rect.y) as usize).min(rect.height.saturating_sub(1) as usize);
    (x, y)
}

impl App {
    pub async fn on_mouse(&mut self, ev: MouseEvent) -> Flow {
        if !self.config.mouse {
            return Flow::Continue;
        }
        if matches!(
            self.mode,
            Mode::Overlay(_)
                | Mode::Prompt(_)
                | Mode::Confirm(..)
                | Mode::Clock { .. }
        ) {
            return Flow::Continue;
        }

        if Some(ev.row) == self.rows.status || self.interaction.border_drag.is_some() {
            return self.on_multiplexer_mouse(ev).await;
        }

        if ev.kind == MouseEventKind::Down(MouseButton::Left) && self.grab_border(ev.column, ev.row) {
            return Flow::Continue;
        }

        if !ev.modifiers.contains(crossterm::event::KeyModifiers::SHIFT)
            && let Some((name, rect)) = self
                .panes
                .iter()
                .find(|(_, rect, _)| rect.contains(ev.column, ev.row))
                .map(|(name, rect, _)| (name.clone(), *rect))
        {
            let modes = self.modes_of(&name);
            if modes.mouse_tracking != crate::keys::MouseTracking::None {
                if name != self.active_pane && matches!(ev.kind, MouseEventKind::Down(_)) {
                    self.park_copy();
                    let focused = self.client.focus(&name).await;
                    self.report("focus", focused);
                    self.layout.dirty = true;
                    self.active_pane = name.clone();
                    self.active_rect = Some(rect);
                }
                let local_col = (ev.column.saturating_sub(rect.x) + 1).min(rect.width);
                let local_row = (ev.row.saturating_sub(rect.y) + 1).min(rect.height);
                if let Some(bytes) = crate::keys::encode_mouse_event(
                    &modes,
                    ev.kind,
                    local_col,
                    local_row,
                    ev.modifiers,
                )
                .filter(|_| !self.attachment.terminal.readonly)
                {
                    let text = String::from_utf8_lossy(&bytes);
                    let _ = self.client.send_input(&name, &text).await;
                    return Flow::Continue;
                }
            }
        }

        self.on_multiplexer_mouse(ev).await
    }

    async fn on_multiplexer_mouse(&mut self, ev: MouseEvent) -> Flow {
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.mouse_press(ev.column, ev.row).await,
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag(ev.column, ev.row).await,
            MouseEventKind::Up(MouseButton::Left) => self.mouse_release(),
            MouseEventKind::ScrollUp => self.mouse_scroll_up().await,
            MouseEventKind::ScrollDown => self.mouse_scroll_down(),
            MouseEventKind::Down(MouseButton::Right) => self.mouse_menu(ev.column, ev.row).await,
            MouseEventKind::Down(MouseButton::Middle) => self.mouse_paste(ev.column, ev.row).await,
            _ => {}
        }
        Flow::Continue
    }

    /// Focuses the pane under the pointer. A click inside a pane in copy mode moves its cursor
    /// and drops the selection without touching the scroll position; anywhere else it only
    /// remembers the point, so a later drag can select from it.
    async fn mouse_press(&mut self, col: u16, row: u16) {
        if Some(row) == self.rows.status {
            let at = col as usize;
            let target = self
                .status_hits.targets
                .iter()
                .find(|(start, end, _)| (*start..*end).contains(&at))
                .map(|(_, _, target)| target.clone());
            match target {
                Some(super::statusline::Target::Window(idx)) => {
                    let selected = self.client.select_window(idx).await;
                    self.report("select-window", selected);
                    self.layout.dirty = true;
                }
                Some(super::statusline::Target::Pane(name)) if name != self.active_pane => {
                    let focused = self.client.focus(&name).await;
                    self.report("focus", focused);
                    self.layout.dirty = true;
                }
                _ => {}
            }
            return;
        }

        if self.grab_border(col, row) {
            return;
        }
        let Some((name, rect)) = self.pane_at(col, row) else {
            return;
        };
        let (x, y) = local_point(rect, col, row);
        if name != self.active_pane {
            self.focus_pane(&name, rect).await;
        }
        if let Mode::Copy(cs) = &mut self.mode {
            let grid = self.vt.get(&name).map(|vt| vt.grid());
            cs.click(x, y, grid);
        }
        self.interaction.press = Some((name, x, y));
    }

    /// Right click: the menu for what is under the pointer, as tmux's `MouseDown3*` bindings.
    /// Status tabs select their window first and the session menu sits on status-left and
    /// status-right; a split border gets the layout menu; a pane is focused, then its menu opens.
    async fn mouse_menu(&mut self, col: u16, row: u16) {
        if self.attachment.terminal.readonly {
            return;
        }
        if Some(row) == self.rows.status {
            let target = self
                .status_hits.targets
                .iter()
                .find(|(start, end, _)| (*start..*end).contains(&(col as usize)))
                .map(|(_, _, target)| target.clone());
            match target {
                Some(super::statusline::Target::Window(idx)) => {
                    let selected = self.client.select_window(idx).await;
                    self.report("select-window", selected);
                    self.layout.dirty = true;
                    let label = self
                        .layout
                        .tabs
                        .iter()
                        .find(|t| t.index == idx)
                        .map(|t| format!("{}:{}", t.index + self.config.base_index, t.name))
                        .unwrap_or_default();
                    self.open_mouse_menu(MouseMenu::Window { label }, col, row).await;
                }
                Some(super::statusline::Target::Pane(name)) => {
                    if let Some(rect) = self.panes.iter().find(|(n, _, _)| *n == name).map(|p| p.1) {
                        self.pane_menu(&name, rect, col, row).await;
                    }
                }
                None => {
                    let (left, right) = self.status_hits.zones;
                    let at = col as usize;
                    if at < left || at >= right {
                        let name = self.session.clone();
                        self.open_mouse_menu(MouseMenu::Session { name }, col, row).await;
                    }
                }
            }
            return;
        }
        if !self.zoomed() && border_at(&self.layout.dividers, &self.panes, (col, row)).is_some() {
            self.open_mouse_menu(MouseMenu::Border, col, row).await;
            return;
        }
        if let Some((name, rect)) = self.pane_at(col, row) {
            self.pane_menu(&name, rect, col, row).await;
        }
    }

    /// The menu of pane `name`, focusing it first. A pane in copy mode keeps its own mouse use.
    async fn pane_menu(&mut self, name: &str, rect: Rect, col: u16, row: u16) {
        if name == self.active_pane && matches!(self.mode, Mode::Copy(_)) {
            return;
        }
        if name != self.active_pane {
            self.focus_pane(name, rect).await;
        }
        let menu = MouseMenu::Pane {
            input_off: self
                .nodes
                .iter()
                .any(|n| n.name == name && n.input_off == Some(true)),
            marked: self.layout.marked_pane() == Some(name),
            zoomed: self.zoomed(),
        };
        self.open_mouse_menu(menu, col, row).await;
    }

    /// Middle click in a pane pastes the top paste buffer into it, as tmux's `MouseDown2Pane`.
    async fn mouse_paste(&mut self, col: u16, row: u16) {
        if self.attachment.terminal.readonly || Some(row) == self.rows.status {
            return;
        }
        let Some((name, rect)) = self.pane_at(col, row) else {
            return;
        };
        if name != self.active_pane {
            self.focus_pane(&name, rect).await;
        }
        if let Ok(cmd) = command::parse("paste-buffer") {
            self.dispatch(cmd).await;
        }
    }

    /// Opens a mouse menu by running its `display-menu` line, as a key binding would.
    async fn open_mouse_menu(&mut self, menu: MouseMenu, col: u16, row: u16) {
        match command::parse(&menu.command_line(col, row)) {
            Ok(cmd) => {
                self.dispatch(cmd).await;
            }
            Err(e) => {
                self.say(e);
            }
        }
    }

    /// The pane whose rectangle holds the cell, with that rectangle.
    fn pane_at(&self, col: u16, row: u16) -> Option<(String, Rect)> {
        self.panes
            .iter()
            .find(|(_, rect, _)| rect.contains(col, row))
            .map(|(name, rect, _)| (name.clone(), *rect))
    }

    /// Makes `name` the active pane, leaving copy mode it was in and restoring its parked copy.
    async fn focus_pane(&mut self, name: &str, rect: Rect) {
        self.park_copy();
        let focused = self.client.focus(name).await;
        self.report("focus", focused);
        self.layout.dirty = true;
        self.active_pane = name.to_string();
        self.active_rect = Some(rect);
        if let Some(cs) = self.parked_copy.remove(name) {
            self.mode = Mode::Copy(cs);
        }
    }

    /// Extends the selection, entering copy mode at the press point on the first drag.
    async fn mouse_drag(&mut self, col: u16, row: u16) {
        if let Some(grab) = self.interaction.border_drag.clone() {
            self.drag_border(&grab, col, row).await;
            return;
        }
        let Some(rect) = self.active_rect else { return };
        if !matches!(self.mode, Mode::Copy(_)) {
            let Some((name, x, y)) = self.interaction.press.clone() else {
                return;
            };
            self.load_scrollback(&name).await;
            let grid = self.vt.get(&name).map(|vt| vt.grid());
            self.mode = Mode::Copy(CopyState::at_point(x, y, grid));
        }
        if let Mode::Copy(cs) = &mut self.mode {
            let (x, y) = local_point(rect, col, row);
            cs.drag_to(x, y);
        }
    }

    /// Copies the selection when it covers something; the selection stays on screen.
    fn mouse_release(&mut self) {
        self.interaction.press = None;
        if self.interaction.border_drag.take().is_some() {
            self.layout.dirty = true;
        }
        let (Mode::Copy(cs), Some(vt)) = (&self.mode, self.active_vt()) else {
            return;
        };
        let grid = vt.grid();
        if cs.is_empty_selection(grid) {
            return;
        }
        let text = cs.yank_text(grid);
        if !text.trim().is_empty() {
            if self.config.set_clipboard {
                self.yank.clipboard.copy(&text);
            }
            self.yank.yanks.push(text.clone());
            self.yank.last_copy = Some(text);
        }
    }

    /// Takes hold of the split border under the pointer, if there is one.
    fn grab_border(&mut self, col: u16, row: u16) -> bool {
        if self.zoomed() {
            return false;
        }
        self.interaction.border_drag = border_at(&self.layout.dividers, &self.panes, (col, row));
        self.interaction.border_drag.is_some()
    }

    /// Moves a held border to the pointer. The layout is refetched before the next mouse event
    /// is handled, so each move starts from where the border really is.
    async fn drag_border(&mut self, grab: &BorderGrab, col: u16, row: u16) {
        if let Some((pane, side, cells)) = border_move(grab, &self.panes, (col, row)) {
            let viewport = self.pane_viewport();
            let resized = self.client.resize_split(&pane, side, cells, viewport).await;
            self.report("resize-pane", resized);
            self.layout.dirty = true;
        }
    }

    async fn mouse_scroll_up(&mut self) {
        if !matches!(self.mode, Mode::Copy(_)) {
            self.load_scrollback(&self.active_pane.clone()).await;
        }
        let hist_len = self.active_vt().map_or(0, |vt| vt.grid().history_len());
        if let Mode::Copy(cs) = &mut self.mode {
            cs.wheel_up(hist_len);
            return;
        }
        let grid = self.active_vt().map(|vt| vt.grid());
        self.mode = Mode::Copy(CopyState::from_wheel(grid, hist_len));
    }

    /// Scrolls copy mode back toward the live screen, leaving it on arrival when the wheel
    /// entered it.
    fn mouse_scroll_down(&mut self) {
        if let Mode::Copy(cs) = &mut self.mode
            && cs.wheel_down()
            && cs.wheel_exit
        {
            self.mode = Mode::Normal;
            self.release_scrollback(&self.active_pane.clone());
        }
    }
}
