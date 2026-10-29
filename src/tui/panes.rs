//! Window layout cache, pane geometry and per-pane terminal sizing.
use std::collections::HashMap;

use crate::client::Client;
use crate::layout::{Divider, Rect, WindowTab};
use crate::protocol::{NodeInfo, Side};
use crate::vt::VtScreen;

/// A visible pane: name, cell rectangle, focused.
pub type PaneRect = (String, Rect, bool);

pub struct LayoutCache {
    pub dirty: bool,
    /// Columns left of the pane area; every rectangle and divider is shifted right by it.
    pub offset: u16,
    pub raw: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
    pub tabs: Vec<WindowTab>,
}

impl LayoutCache {
    pub fn new() -> Self {
        Self {
            dirty: true,
            offset: 0,
            raw: Vec::new(),
            dividers: Vec::new(),
            tabs: Vec::new(),
        }
    }

    /// Refetches the layout for a `width` x `layout_h` area starting `offset` columns in, when
    /// dirty or moved. Returns false when the window has no panes at all.
    pub async fn sync(&mut self, client: &mut Client, offset: u16, width: u16, layout_h: u16) -> bool {
        if !(self.dirty || self.raw.is_empty() || offset != self.offset) {
            return !self.raw.is_empty();
        }
        // No answer means the daemon is gone (killed, or it ended with its last pane): nothing is
        // left to show.
        let Ok((panes, dividers, tabs)) = client.layout(width, layout_h).await else {
            self.raw.clear();
            return false;
        };
        self.raw = panes
            .into_iter()
            .map(|(name, rect, focused)| (name, shift(rect, offset), focused))
            .collect();
        self.dividers = dividers
            .into_iter()
            .map(|d| Divider { x: d.x + offset, ..d })
            .collect();
        self.tabs = tabs;
        self.offset = offset;
        self.dirty = false;
        !self.raw.is_empty()
    }

    /// The name of the marked pane if it is in the active window.
    pub fn marked_pane(&self) -> Option<&str> {
        self.tabs
            .iter()
            .find(|t| t.is_active)
            .and_then(|t| t.marked_pane.as_deref())
    }

    /// The name of the marked pane across all windows, if any is marked.
    pub fn marked_pane_global(&self) -> Option<&str> {
        self.tabs.iter().find_map(|t| t.marked_pane.as_deref())
    }
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

fn shift(r: Rect, dx: u16) -> Rect {
    Rect::new(r.x + dx, r.y, r.width, r.height)
}

fn center(r: Rect) -> (i32, i32) {
    (
        r.x as i32 + r.width as i32 / 2,
        r.y as i32 + r.height as i32 / 2,
    )
}

/// True when `candidate` lies on the `side` of `cur` (touching or beyond its edge).
fn beyond(cur: Rect, candidate: Rect, side: Side) -> bool {
    match side {
        Side::Left => candidate.x + candidate.width <= cur.x + 2,
        Side::Right => candidate.x + 2 >= cur.x + cur.width,
        Side::Up => candidate.y + candidate.height <= cur.y + 2,
        Side::Down => candidate.y + 2 >= cur.y + cur.height,
    }
}

/// Nearest pane in `side`, weighting the cross-axis distance three times more than the main axis.
pub fn find_directional(panes: &[PaneRect], active: &str, side: Side) -> Option<String> {
    let cur = panes.iter().find(|(name, ..)| name == active)?.1;
    let (cx, cy) = center(cur);
    panes
        .iter()
        .filter(|(name, ..)| name != active)
        .filter(|(_, rect, _)| beyond(cur, *rect, side))
        .map(|(name, rect, _)| {
            let (ox, oy) = center(*rect);
            let (dx, dy) = ((cx - ox).abs(), (cy - oy).abs());
            let score = match side {
                Side::Left | Side::Right => dx + dy * 3,
                Side::Up | Side::Down => dy + dx * 3,
            };
            (score, name)
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, name)| name.clone())
}

/// The previous or next pane in layout order, wrapping around.
pub fn neighbor_pane(panes: &[PaneRect], active: &str, next: bool) -> Option<String> {
    let idx = panes.iter().position(|(name, ..)| name == active)?;
    neighbor(panes, idx, next).map(|(name, ..)| name.clone())
}

/// The item after (`next`) or before the one at `idx`, wrapping around; none without another.
pub fn neighbor<T>(items: &[T], idx: usize, next: bool) -> Option<&T> {
    (items.len() > 1).then(|| &items[wrap_step(idx, items.len(), next)])
}

/// The index after (`next`) or before `idx` in a list of `len` items, wrapping around.
pub fn wrap_step(idx: usize, len: usize, next: bool) -> usize {
    if next {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    }
}

/// Panes whose cell size differs from the last size sent to the daemon, as `(name, cols, rows)`.
/// Zero-sized panes are never resized.
pub fn pending_resizes(
    panes: &[PaneRect],
    dims: &HashMap<String, (u16, u16)>,
) -> Vec<(String, u16, u16)> {
    panes
        .iter()
        .filter(|(name, rect, _)| {
            rect.width > 0 && rect.height > 0 && dims.get(name) != Some(&(rect.width, rect.height))
        })
        .map(|(name, rect, _)| (name.clone(), rect.width, rect.height))
        .collect()
}

/// Rebuilds a pane from its log into a fresh screen of the given size, with scrollback when
/// `scrollback` is set. Returns the screen and the byte offset its contents end at, which is the
/// next offset expected from the event stream.
pub async fn load_pane(
    client: &mut Client,
    name: &str,
    (width, height): (u16, u16),
    scrollback: bool,
) -> Option<(VtScreen, u64)> {
    let (logs, next) = client.logs(name, None, false, None).await.ok()?;
    let mut screen = if scrollback {
        VtScreen::new(width as usize, height as usize)
    } else {
        VtScreen::live(width as usize, height as usize)
    };
    screen.feed(logs.as_bytes());
    let _ = screen.take_passthroughs();
    // The log holds only recent output: a mode the program set before it (focus events, cursor
    // keys) comes from the daemon's screen, which has seen everything. Asked after the log, so a
    // change in between also arrives on the stream past `next`.
    if let Ok(Some(modes)) = client.status(name).await.map(|info| info.modes) {
        screen.set_modes(modes);
    }
    Some((screen, next))
}

/// Whether a pane's screen holds scrollback (it is in copy mode).
pub fn has_scrollback(screen: &VtScreen) -> bool {
    screen.grid().history_limit > 0
}

/// Resizes panes whose cell size changed and reloads their scrollback into fresh screens,
/// recording each reloaded pane's expected stream offset.
pub async fn sync_dims(
    client: &mut Client,
    panes: &[PaneRect],
    dims: &mut HashMap<String, (u16, u16)>,
    vt: &mut HashMap<String, VtScreen>,
    expected: &mut HashMap<String, u64>,
) {
    for (name, width, height) in pending_resizes(panes, dims) {
        dims.insert(name.clone(), (width, height));
        let _ = client.resize_pane(&name, width, height).await;
        let scrollback = vt.get(&name).is_some_and(has_scrollback);
        if let Some((screen, next)) = load_pane(client, &name, (width, height), scrollback).await {
            expected.insert(name.clone(), next);
            vt.insert(name, screen);
        }
    }
}

/// Panes whose terminal is not the size last sent to it: another client or `mpx resize-pane`
/// resized it since. Their size is to be sent again.
pub fn resized_elsewhere(dims: &HashMap<String, (u16, u16)>, nodes: &[NodeInfo]) -> Vec<String> {
    nodes
        .iter()
        .filter(|node| {
            node.size
                .is_some_and(|size| dims.get(&node.name).is_some_and(|sent| *sent != size))
        })
        .map(|node| node.name.clone())
        .collect()
}

/// A split border grabbed with the mouse, and the panes on either side of it where it was grabbed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderGrab {
    /// A `│` border between panes side by side; otherwise a `─` border between stacked panes.
    pub vertical: bool,
    pub before: String,
    pub after: String,
}

/// The border under `(col, row)`, if any, with the panes it separates at that point.
pub fn border_at(
    dividers: &[Divider],
    panes: &[PaneRect],
    (col, row): (u16, u16),
) -> Option<BorderGrab> {
    let divider = dividers.iter().find(|d| {
        let (along, at, start) = if d.is_vertical {
            (row, col == d.x, d.y)
        } else {
            (col, row == d.y, d.x)
        };
        at && along >= start && along < start + d.length
    })?;
    let pane_at = |x: u16, y: u16| {
        panes
            .iter()
            .find(|(_, rect, _)| rect.contains(x, y))
            .map(|(name, ..)| name.clone())
    };
    let (before, after) = if divider.is_vertical {
        (pane_at(col.checked_sub(1)?, row)?, pane_at(col + 1, row)?)
    } else {
        (pane_at(col, row.checked_sub(1)?)?, pane_at(col, row + 1)?)
    };
    Some(BorderGrab {
        vertical: divider.is_vertical,
        before,
        after,
    })
}

/// The resize that brings a grabbed border to `(col, row)`: the pane on the side the pointer
/// moved to grows toward it. `None` when the border is already there.
pub fn border_move(
    grab: &BorderGrab,
    panes: &[PaneRect],
    (col, row): (u16, u16),
) -> Option<(String, Side, u16)> {
    let (_, rect, _) = panes.iter().find(|(name, ..)| *name == grab.before)?;
    let (edge, to) = if grab.vertical {
        (rect.x + rect.width, col)
    } else {
        (rect.y + rect.height, row)
    };
    let cells = to.abs_diff(edge);
    match (to > edge, grab.vertical) {
        _ if cells == 0 => None,
        (true, true) => Some((grab.before.clone(), Side::Right, cells)),
        (true, false) => Some((grab.before.clone(), Side::Down, cells)),
        (false, true) => Some((grab.after.clone(), Side::Left, cells)),
        (false, false) => Some((grab.after.clone(), Side::Up, cells)),
    }
}
