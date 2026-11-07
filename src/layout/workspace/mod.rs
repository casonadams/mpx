mod alerts;
mod arrange;
mod links;
mod navigation;
mod panes;
mod tabs;
mod window;
mod windows;

use std::collections::BTreeSet;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::tree::LayoutNode;
use crate::protocol::LayoutPreset;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowTab {
    pub index: usize,
    pub name: String,
    pub is_active: bool,
    #[serde(default)]
    pub is_last: bool,
    #[serde(default)]
    pub has_activity: bool,
    #[serde(default)]
    pub has_bell: bool,
    #[serde(default)]
    pub has_silence: bool,
    /// The window's panes all receive typed input (tmux `synchronize-panes`).
    #[serde(default)]
    pub synchronized: bool,
    #[serde(default)]
    pub has_marked: bool,
    #[serde(default)]
    pub marked_pane: Option<String>,
    #[serde(default)]
    pub agent_badge: Option<String>,
    /// The window is zoomed: its focused pane fills the window's area (tmux `Z` flag).
    #[serde(default)]
    pub zoomed: bool,
    /// The window's active pane (tmux `pane_active`), whether or not the window is shown.
    #[serde(default)]
    pub focused: Option<String>,
    /// The window's panes other than the active one, most recently focused first.
    #[serde(default)]
    pub recent: Vec<String>,
    /// Every index the window is linked at, this one included, in number order (tmux
    /// `window_linked`).
    #[serde(default)]
    pub linked: Vec<usize>,
}

/// A window: its panes, split tree, focus and name. Every index it is linked at shows this one
/// window, so a pane killed here is gone from all of them (tmux `struct window`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: usize,
    pub name: String,
    /// Name given at creation; automatic naming falls back to it.
    #[serde(default)]
    pub label: String,
    /// Set by an explicit rename; disables automatic naming.
    #[serde(default)]
    pub renamed: bool,
    pub root: Option<LayoutNode>,
    pub focused: Option<String>,
    /// Panes focused before the current one, most recently focused first.
    #[serde(default)]
    pub focus_history: Vec<String>,
    /// Preset most recently applied by `select-layout`, used to cycle.
    #[serde(default)]
    pub preset: Option<LayoutPreset>,
    /// The window's panes all receive typed input (tmux `synchronize-panes`).
    #[serde(default)]
    pub synchronized: bool,
    /// Zoomed: only the focused pane is shown. Any change to the layout unzooms the window.
    #[serde(default)]
    pub zoomed: bool,
    /// The size `resize-window` set, which replaces the size the clients give (tmux `window-size
    /// manual`). Not persisted: a restored window follows its clients again.
    #[serde(skip)]
    pub manual_size: Option<(u16, u16)>,
}

/// A window's surviving pane tree and the pane focus should land on when it is restored.
pub struct RestoredLayout {
    pub root: LayoutNode,
    pub focused: Option<String>,
}

/// One index of the session (tmux winlink): the window shown there, and the alerts raised there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Identifies the index across renumbering, so a change can tell which links were added or
    /// removed.
    pub id: usize,
    /// The number `select-window` and friends address. Numbers are unique and ascending; with
    /// renumber-windows on they are the positions.
    pub number: usize,
    /// Id of the window shown at this index.
    pub window: usize,
    #[serde(default)]
    pub has_activity: bool,
    #[serde(default)]
    pub has_bell: bool,
    /// The window has been quiet for `monitor-silence` seconds while in the background (tmux `~`).
    #[serde(default)]
    pub has_silence: bool,
    /// When the window last printed output; `None` until it does. Not persisted.
    #[serde(skip)]
    pub last_output: Option<Instant>,
}

/// Where `link-window` puts a new index and how it treats the one it is given (tmux `link-window`
/// flags `-a`, `-b`, `-k` and `-d`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkPlace {
    /// Insert after the given index (`-a`).
    pub after: bool,
    /// Insert before the given index (`-b`).
    pub before: bool,
    /// Replace the window at an occupied index, unlinking it (`-k`).
    pub kill: bool,
    /// Keep the active index as it is (`-d`).
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// Every window, once each, however many indices show it.
    pub windows: Vec<Window>,
    /// The session's indices, in number order.
    pub links: Vec<Link>,
    /// Position in `links` of the active index.
    pub active_idx: usize,
    pub next_window_id: usize,
    pub next_link_id: usize,
    /// Position in `links` of the index that was active before the current one.
    #[serde(default)]
    pub last_window: Option<usize>,
    #[serde(default)]
    pub marked_pane: Option<String>,
    /// Panes whose input is off (tmux `select-pane -d`); they drop keys sent to them.
    #[serde(default)]
    pub input_off: BTreeSet<String>,
    /// Whether windows created from now on start with `synchronize-panes` on (config `synchronize_panes`).
    #[serde(skip)]
    synchronize_new: bool,
    /// Whether closing a window renumbers the windows after it (tmux `renumber-windows`).
    #[serde(skip)]
    renumber_windows: bool,
}

impl Workspace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            windows: Vec::new(),
            links: Vec::new(),
            active_idx: 0,
            next_window_id: 0,
            next_link_id: 0,
            last_window: None,
            marked_pane: None,
            input_off: BTreeSet::new(),
            renumber_windows: true,
            synchronize_new: false,
        }
    }

    /// Whether windows created from now on start with `synchronize-panes` on.
    pub fn set_synchronize_new(&mut self, on: bool) {
        self.synchronize_new = on;
    }

    /// Whether closing a window renumbers the windows after it (tmux `renumber-windows`).
    pub fn set_renumber_windows(&mut self, on: bool) {
        self.renumber_windows = on;
    }

    pub fn active_window(&self) -> Option<&Window> {
        self.window_at(self.active_idx)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        let pos = self.active_idx;
        self.window_at_mut(pos)
    }

    /// The number of the active index, if there is one.
    pub fn active_number(&self) -> Option<usize> {
        self.links.get(self.active_idx).map(|l| l.number)
    }

    /// The panes of the window at index `number`, empty when there is no such index.
    pub fn leaves_at(&self, number: usize) -> Vec<String> {
        self.window_numbered(number)
            .map(Window::leaves)
            .unwrap_or_default()
    }

    /// The focused pane of the window at index `number`.
    pub fn focused_at(&self, number: usize) -> Option<String> {
        self.window_numbered(number).and_then(|w| w.focused.clone())
    }

    /// The position in `links` of the index showing pane `name`: the active index when it shows it,
    /// else the first index that does.
    pub fn window_of(&self, name: &str) -> Option<usize> {
        let shows = |pos: usize| self.window_at(pos).is_some_and(|w| w.contains(name));
        if shows(self.active_idx) {
            return Some(self.active_idx);
        }
        (0..self.links.len()).find(|&pos| shows(pos))
    }

    /// The number of the index showing pane `name`.
    pub fn number_of(&self, name: &str) -> Option<usize> {
        self.window_of(name).map(|pos| self.links[pos].number)
    }

    /// The position in `links` of the index `number`.
    fn position_of(&self, number: usize) -> Option<usize> {
        self.links.binary_search_by_key(&number, |l| l.number).ok()
    }

    fn position_or_err(&self, number: usize) -> Result<usize, String> {
        self.position_of(number)
            .ok_or_else(|| window_not_found(number))
    }

    /// The id of the window that holds pane `name`.
    fn window_id_of(&self, name: &str) -> Option<usize> {
        self.window_of(name).map(|pos| self.links[pos].window)
    }

    fn window_by_id(&self, id: usize) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn window_by_id_mut(&mut self, id: usize) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The window shown at position `pos` of `links`.
    fn window_at(&self, pos: usize) -> Option<&Window> {
        self.links
            .get(pos)
            .and_then(|link| self.window_by_id(link.window))
    }

    fn window_at_mut(&mut self, pos: usize) -> Option<&mut Window> {
        let id = self.links.get(pos)?.window;
        self.window_by_id_mut(id)
    }

    /// The window shown at index `number`.
    fn window_numbered(&self, number: usize) -> Option<&Window> {
        let pos = self.position_of(number)?;
        self.window_at(pos)
    }

    fn window_numbered_mut(&mut self, number: usize) -> Option<&mut Window> {
        let pos = self.position_of(number)?;
        self.window_at_mut(pos)
    }

    /// The window at index `number` (the active one when `None`), for a change to it.
    fn window_mut_at(&mut self, number: Option<usize>) -> Result<&mut Window, String> {
        match number {
            Some(n) => self
                .window_numbered_mut(n)
                .ok_or_else(|| window_not_found(n)),
            None => self
                .active_window_mut()
                .ok_or_else(|| "No window".to_string()),
        }
    }

    /// Runs `change` on the active window; false when there is none.
    fn in_active_window(&mut self, change: impl FnOnce(&mut Window)) -> bool {
        match self.active_window_mut() {
            Some(win) => {
                change(win);
                true
            }
            None => false,
        }
    }
}

/// The error for a window number that no window has.
pub fn window_not_found(number: usize) -> String {
    format!("No window {number}")
}
