use super::{Link, LinkPlace, Workspace, window_not_found};

impl Link {
    fn new(id: usize, number: usize, window: usize) -> Self {
        Self {
            id,
            number,
            window,
            has_activity: false,
            has_bell: false,
            has_silence: false,
            last_output: None,
        }
    }
}

impl Workspace {
    /// The lowest number no index has: the first gap in the ascending numbers.
    pub(super) fn lowest_free_number(&self) -> usize {
        self.links
            .iter()
            .enumerate()
            .find(|(i, l)| l.number != *i)
            .map_or(self.links.len(), |(i, _)| i)
    }

    /// The number a window restored or added at `wanted` takes: `wanted` when it is free and
    /// renumber-windows is off, else the lowest free one.
    pub(super) fn number_for(&self, wanted: usize) -> usize {
        if self.renumber_windows || self.position_of(wanted).is_some() {
            self.lowest_free_number()
        } else {
            wanted
        }
    }

    /// Adds an index showing window `window` at number `number`, in number order. Returns its
    /// position.
    pub(super) fn insert_link(&mut self, window: usize, number: usize) -> usize {
        let id = self.next_link_id;
        self.next_link_id += 1;
        let pos = self.links.partition_point(|l| l.number < number);
        let link = Link::new(id, number, window);
        self.reorder(|links| links.insert(pos, link));
        pos
    }

    /// Shows the window `window` (an id from `window_id_at`) at index `number`, numbered as
    /// `install_window` numbers windows. Returns the index it took.
    pub fn link_restored(&mut self, window: usize, number: usize) -> usize {
        let number = self.number_for(number);
        let pos = self.insert_link(window, number);
        self.links[pos].number
    }

    /// The id of the window shown at index `number`.
    pub fn window_id_at(&self, number: usize) -> Option<usize> {
        self.position_of(number).map(|pos| self.links[pos].window)
    }

    /// Changes the order of the links, keeping the active and last index on their links.
    fn reorder(&mut self, change: impl FnOnce(&mut Vec<Link>)) {
        let (active, last) = self.link_ids();
        change(&mut self.links);
        self.restore_positions(active, last);
    }

    /// The ids of the active and last links, to find them again after `links` changes.
    fn link_ids(&self) -> (Option<usize>, Option<usize>) {
        let active = self.links.get(self.active_idx).map(|l| l.id);
        let last = self
            .last_window
            .and_then(|i| self.links.get(i))
            .map(|l| l.id);
        (active, last)
    }

    /// Points the active and last indices at the links with ids `active` and `last`. When the active
    /// link is gone, the last link becomes active.
    fn restore_positions(&mut self, active: Option<usize>, last: Option<usize>) {
        let position = |id: Option<usize>, links: &[Link]| {
            id.and_then(|id| links.iter().position(|l| l.id == id))
        };
        let fallback = self.links.len().saturating_sub(1);
        self.active_idx = position(active, &self.links).unwrap_or(fallback);
        self.last_window = position(last, &self.links);
    }

    /// Numbers the indices 0, 1, ... in order (tmux `move-window -r`).
    pub fn compact_numbers(&mut self) {
        for (i, link) in self.links.iter_mut().enumerate() {
            link.number = i;
        }
    }

    pub(super) fn renumber_if_enabled(&mut self) {
        if self.renumber_windows {
            self.compact_numbers();
        }
    }

    /// Removes the links at the positions `gone` (ascending). The active and last indices move with
    /// their links; when the active one is removed, the index that takes its place becomes active.
    pub(super) fn unlink_positions(&mut self, gone: &[usize]) {
        let shift = |p: usize| p - gone.iter().filter(|&&g| g < p).count();
        let active = shift(self.active_idx);
        let last = self.last_window.filter(|l| !gone.contains(l)).map(shift);
        let mut pos = 0;
        self.links.retain(|_| {
            let keep = !gone.contains(&pos);
            pos += 1;
            keep
        });
        self.active_idx = active.min(self.links.len().saturating_sub(1));
        self.last_window = last;
    }

    /// Removes one index (`pos` in `links`). The window it showed is killed when that was its only
    /// index, which needs `kill`; returns the panes of a killed window.
    fn unlink_at(&mut self, pos: usize, kill: bool) -> Result<Vec<String>, String> {
        let (id, number) = (self.links[pos].window, self.links[pos].number);
        if self.links.iter().filter(|l| l.window == id).count() > 1 {
            self.unlink_positions(&[pos]);
            return Ok(Vec::new());
        }
        if !kill {
            return Err(format!(
                "window {number} is linked to no other index: use -k to kill it"
            ));
        }
        Ok(self.kill_body(id))
    }

    /// Removes the index `number` (tmux `unlink-window`). Its window stays at its other indices;
    /// a window with no other index is killed only with `kill`. Returns the panes of a killed
    /// window.
    pub fn unlink_window(&mut self, number: usize, kill: bool) -> Result<Vec<String>, String> {
        let pos = self.position_or_err(number)?;
        let panes = self.unlink_at(pos, kill)?;
        self.renumber_if_enabled();
        Ok(panes)
    }

    /// Shows the window at index `src` at index `dst` as well (tmux `link-window`). Without `dst` the
    /// lowest free index is taken. With `after` or `before`, the window goes next to `dst` and the
    /// indices in the way move up. An occupied index is refused unless `kill`, which unlinks what is
    /// there first. Returns the new index and the panes of a window that `kill` destroyed.
    pub fn link_window(
        &mut self,
        src: usize,
        dst: Option<usize>,
        place: LinkPlace,
    ) -> Result<(usize, Vec<String>), String> {
        let src_pos = self.position_or_err(src)?;
        let window = self.links[src_pos].window;
        let shuffles = place.after || place.before;
        if !shuffles && dst == Some(src) {
            return Err(format!("same index: {src}"));
        }
        let number = if shuffles {
            let base = dst.or_else(|| self.active_number()).unwrap_or(0);
            self.open_index(base + usize::from(place.after))
        } else {
            dst.unwrap_or_else(|| self.lowest_free_number())
        };
        let mut destroyed = Vec::new();
        if let Some(pos) = self.position_of(number) {
            if !place.kill {
                return Err(format!("index in use: {number}"));
            }
            destroyed = self.unlink_at(pos, true)?;
        }
        let pos = self.insert_link(window, number);
        if !place.detached {
            self.set_active(pos);
        }
        Ok((number, destroyed))
    }

    /// Frees index `start` for a new link: the run of occupied indices from `start` moves up one
    /// (tmux `winlink_shuffle_up`). Returns `start`.
    fn open_index(&mut self, start: usize) -> usize {
        let mut end = start;
        while self.position_of(end).is_some() {
            end += 1;
        }
        for link in self.links.iter_mut() {
            if (start..end).contains(&link.number) {
                link.number += 1;
            }
        }
        start
    }

    /// Drops the windows left without panes, with every index that showed them.
    pub(super) fn drop_empty_windows(&mut self) {
        let (active, last) = self.link_ids();
        let emptied: Vec<usize> = self
            .windows
            .iter()
            .filter(|w| w.root.is_none())
            .map(|w| w.id)
            .collect();
        self.windows.retain(|w| w.root.is_some());
        self.links.retain(|l| !emptied.contains(&l.window));
        self.restore_positions(active, last);
        if !emptied.is_empty() {
            self.renumber_if_enabled();
        }
    }

    /// Exchanges the numbers of two windows (tmux `swap-window`); the active and last index stay on
    /// the same windows.
    pub fn swap_windows(&mut self, a: usize, b: usize) -> Result<(), String> {
        let pa = self.position_or_err(a)?;
        let pb = self.position_or_err(b)?;
        if pa != pb {
            self.reorder(|links| {
                let number = links[pa].number;
                links[pa].number = links[pb].number;
                links[pb].number = number;
                links.sort_by_key(|l| l.number);
            });
        }
        Ok(())
    }

    /// Moves index `from` to number `to` (tmux `move-window`). With renumber-windows on, `to` is a
    /// position and the indices are renumbered afterwards; otherwise `to` must be free.
    pub fn move_window(&mut self, from: usize, to: usize) -> Result<(), String> {
        let from_pos = self.position_or_err(from)?;
        if self.renumber_windows {
            if to >= self.links.len() {
                return Err(window_not_found(to));
            }
            self.reorder(|links| {
                let link = links.remove(from_pos);
                links.insert(to, link);
            });
            self.compact_numbers();
        } else if from != to {
            if self.position_of(to).is_some() {
                return Err(format!("Window {to} already exists"));
            }
            self.reorder(|links| {
                links[from_pos].number = to;
                links.sort_by_key(|l| l.number);
            });
        }
        Ok(())
    }
}
