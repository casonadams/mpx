use super::{Link, Window, WindowTab, Workspace};

impl Workspace {
    /// Every index with the window it shows, in number order.
    pub fn linked(&self) -> impl Iterator<Item = (&Link, &Window)> + '_ {
        self.links
            .iter()
            .filter_map(|link| Some((link, self.window_by_id(link.window)?)))
    }

    pub fn window_tabs(&self) -> Vec<WindowTab> {
        self.links
            .iter()
            .enumerate()
            .filter_map(|(i, link)| {
                let w = self.window_by_id(link.window)?;
                let marked_pane = self.marked_pane.as_ref().filter(|m| w.contains(m)).cloned();
                Some(WindowTab {
                    index: link.number,
                    name: w.name.clone(),
                    is_active: i == self.active_idx,
                    is_last: Some(i) == self.last_window,
                    has_activity: link.has_activity,
                    has_bell: link.has_bell,
                    has_silence: link.has_silence,
                    synchronized: w.synchronized,
                    zoomed: w.zoomed,
                    focused: w.focused.clone(),
                    has_marked: marked_pane.is_some(),
                    marked_pane,
                    agent_badge: None,
                    linked: self.numbers_of(link.window),
                })
            })
            .collect()
    }

    /// The numbers of every index showing window `id`, in number order.
    fn numbers_of(&self, id: usize) -> Vec<usize> {
        self.links
            .iter()
            .filter(|l| l.window == id)
            .map(|l| l.number)
            .collect()
    }
}
