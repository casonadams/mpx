//! Window layout cache, pane geometry and per-pane terminal sizing.
use std::collections::HashMap;

use crate::client::Client;
use crate::layout::{Divider, Rect, WindowTab};
use crate::protocol::Side;
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
        if (self.dirty || self.raw.is_empty() || offset != self.offset)
            && let Ok((panes, dividers, tabs)) = client.layout(width, layout_h).await
        {
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
        }
        !self.raw.is_empty()
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
    let len = panes.len();
    let idx = panes.iter().position(|(name, ..)| name == active)?;
    if len < 2 {
        return None;
    }
    let other = if next {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    };
    Some(panes[other].0.clone())
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

/// Loads a pane's scrollback into a fresh screen of the given size. Returns the screen and the
/// byte offset its contents end at, which is the next offset expected from the event stream.
pub async fn load_pane(
    client: &mut Client,
    name: &str,
    width: u16,
    height: u16,
) -> Option<(VtScreen, u64)> {
    let (logs, next) = client.logs(name, None, false, None).await.ok()?;
    let mut screen = VtScreen::new(width as usize, height as usize);
    screen.feed(logs.as_bytes());
    Some((screen, next))
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
        if let Some((screen, next)) = load_pane(client, &name, width, height).await {
            expected.insert(name.clone(), next);
            vt.insert(name, screen);
        }
    }
}
