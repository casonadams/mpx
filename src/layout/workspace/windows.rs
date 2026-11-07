use super::{RestoredLayout, Window, Workspace};

impl Workspace {
    pub fn new_window(&mut self, window_name: String, root_node: String) -> usize {
        self.new_window_with_focus(window_name, root_node, true)
    }

    /// Adds a window at the lowest free number and returns that number.
    pub fn new_window_with_focus(
        &mut self,
        window_name: String,
        root_node: String,
        focus: bool,
    ) -> usize {
        let number = self.lowest_free_number();
        let id = self.push_window(|id| Window::new(id, window_name, root_node));
        let pos = self.insert_link(id, number);
        if focus {
            self.set_active(pos);
        }
        number
    }

    /// Adds a window with a restored layout, numbered `number` unless renumber-windows is on or that
    /// number is taken, then the lowest free one. `sticky` keeps a user-chosen name; otherwise `label`
    /// names it until automatic naming takes over. Focus falls back to the first pane. Returns the
    /// window's number.
    pub fn install_window(
        &mut self,
        number: usize,
        label: String,
        sticky: Option<String>,
        layout: RestoredLayout,
    ) -> usize {
        let RestoredLayout { root, focused } = layout;
        let focused = focused
            .filter(|f| root.has_leaf(f))
            .or_else(|| root.all_leaves().into_iter().next());
        let id = self.push_window(|id| {
            let mut win = Window::with_layout(id, label, root, focused);
            if let Some(name) = sticky {
                win.name = name;
                win.renamed = true;
            }
            win
        });
        self.link_restored(id, number)
    }

    /// Kills the window at index `number` with every index it is linked at, and returns its panes.
    /// With renumber-windows on, the indices after it are renumbered.
    pub fn kill_window(&mut self, number: usize) -> Option<Vec<String>> {
        let pos = self.position_of(number)?;
        let id = self.links[pos].window;
        let panes = self.kill_body(id);
        self.renumber_if_enabled();
        Some(panes)
    }

    /// Removes the window `id` and every link to it. Returns its panes.
    pub(super) fn kill_body(&mut self, id: usize) -> Vec<String> {
        let panes = self
            .window_by_id(id)
            .map(Window::leaves)
            .unwrap_or_default();
        self.forget_panes(|p| panes.iter().any(|x| x == p));
        let gone: Vec<usize> = self
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| l.window == id)
            .map(|(pos, _)| pos)
            .collect();
        self.windows.retain(|w| w.id != id);
        self.unlink_positions(&gone);
        panes
    }

    pub fn rename_window(&mut self, number: usize, name: String) -> bool {
        let Some(win) = self.window_numbered_mut(number) else {
            return false;
        };
        win.name = name;
        win.renamed = true;
        true
    }

    /// Sets the automatic name of the window at index `number` unless it was renamed explicitly.
    pub fn set_auto_name(&mut self, number: usize, name: Option<String>) {
        if let Some(win) = self.window_numbered_mut(number).filter(|w| !w.renamed) {
            win.name = name.unwrap_or_else(|| win.label.clone());
        }
    }

    /// Turns `synchronize-panes` on or off for the window at index `number` (the active window when
    /// `None`); `on` None toggles it.
    pub fn synchronize(&mut self, number: Option<usize>, on: Option<bool>) -> Result<(), String> {
        let win = self.window_mut_at(number)?;
        win.synchronized = on.unwrap_or(!win.synchronized);
        Ok(())
    }

    /// Adds a window, built from its id, and returns the id.
    pub(super) fn push_window(&mut self, make: impl FnOnce(usize) -> Window) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        let mut window = make(id);
        window.synchronized = self.synchronize_new;
        self.windows.push(window);
        id
    }
}
