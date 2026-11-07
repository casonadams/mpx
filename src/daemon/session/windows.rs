//! Window requests: navigation, naming, moving, linking, resizing, and the layout a client draws.

use super::Placement;
use super::Session;
use super::answer::{check, connection_only, node_reply, reply, unit};
use crate::clients::{WindowSize, effective_size};
use crate::daemon::clients;
use crate::layout::window_not_found;
use crate::node::process::basename;
use crate::protocol::{
    LinkOpts, NewWindowOpts, ResizeOpts, Response, Side, UnlinkOpts, WindowFit, WindowRequest,
    WindowResize,
};

/// The largest size `resize-window` gives a window (tmux `WINDOW_MAXIMUM`).
const MAX_WINDOW_SIZE: u16 = 10_000;

/// `v` less `by` cells, or `v` itself when it is smaller than that (tmux `resize-window -L`).
fn shrink(v: u16, by: u16) -> u16 {
    if v >= by { v - by } else { v }
}

impl Session {
    /// Window navigation, naming, moving and reordering.
    pub(super) async fn window_request(&mut self, req: WindowRequest) -> Response {
        match req {
            WindowRequest::New(opts) => self.new_window(opts).await,
            WindowRequest::Next => {
                self.workspace.next_window();
                Response::Ok
            }
            WindowRequest::Prev => {
                self.workspace.prev_window();
                Response::Ok
            }
            WindowRequest::Last => check(self.workspace.last_window_switch(), "No last window"),
            WindowRequest::Select { index } => check(
                self.workspace.select_window(index),
                &window_not_found(index),
            ),
            WindowRequest::Rename { index, name } => self.rename_window(index, name),
            WindowRequest::Move { from, to } => unit(self.workspace.move_window(from, to)),
            WindowRequest::Renumber => {
                self.workspace.compact_numbers();
                Response::Ok
            }
            WindowRequest::Rotate { down, keep_zoom } => {
                check(self.workspace.rotate_active(down, keep_zoom), "No window")
            }
            WindowRequest::Swap { a, b } => unit(self.workspace.swap_windows(a, b)),
            WindowRequest::Respawn {
                index,
                kill,
                cwd,
                command,
            } => self.respawn_window(index, kill, cwd, command),
            WindowRequest::LinkWindow(req) => self.link_window(req),
            WindowRequest::UnlinkWindow(req) => self.unlink_window(req),
            WindowRequest::ResizeWindow(req) => self.resize_window(req),
            WindowRequest::Kill { .. } => connection_only(),
        }
    }

    async fn new_window(&mut self, mut opts: NewWindowOpts) -> Response {
        self.resolve_command(&mut opts.spawn);
        let named = opts.name.is_some();
        let label = opts.name.unwrap_or_else(|| basename(&opts.spawn.command));
        let place = Placement::Window {
            label,
            focus: opts.focus,
            named,
        };
        node_reply(self.add_node(opts.spawn, place).await)
    }

    /// The window layout a client asks for at `width` by `height`, with the window tabs.
    pub(super) async fn layout(&mut self, width: u16, height: u16) -> Response {
        self.refresh_window_names().await;
        self.requested = (width, height);
        let (width, height) = match self.workspace.manual_size_at(None) {
            Some(size) => size,
            None => self.window_size((width, height)),
        };
        let (rects, dividers) = self.workspace.compute_layout_with_dividers(width, height);
        Response::Layout(rects, dividers, self.workspace.window_tabs())
    }

    /// Sets each window's automatic name from its focused pane's label, when `automatic-rename` is on.
    async fn refresh_window_names(&mut self) {
        if !self.config.window.automatic_rename {
            return;
        }
        let windows: Vec<(usize, Option<String>)> = self
            .workspace
            .linked()
            .map(|(link, w)| (link.number, w.focused.clone()))
            .collect();
        for (number, focused) in windows {
            let handle = focused
                .and_then(|f| self.nodes.get(&f))
                .map(|e| e.handle.clone());
            let label = match handle {
                Some(handle) => handle.label().await.ok().flatten(),
                None => None,
            };
            self.workspace.set_auto_name(number, label);
        }
    }

    fn rename_window(&mut self, index: Option<usize>, name: String) -> Response {
        match self.window_index(index) {
            Ok(number) => check(
                self.workspace.rename_window(number, name),
                &window_not_found(number),
            ),
            Err(e) => Response::Error(e),
        }
    }

    /// The index a window request names, or the active one when it names none.
    pub(super) fn window_index(&self, index: Option<usize>) -> Result<usize, String> {
        index
            .or_else(|| self.workspace.active_number())
            .ok_or_else(|| "No window".to_string())
    }

    /// Answers a window request that may have destroyed windows: their panes are killed, and their
    /// exits are reported as usual.
    fn killed_panes(&mut self, result: Result<Vec<String>, String>) -> Response {
        reply(result, |panes| {
            for pane in panes {
                let _ = self.kill_entry(&pane);
            }
            Response::Ok
        })
    }

    /// `link-window`: shows window `src` at index `dst` as well. The panes of a window the link
    /// destroyed are killed.
    fn link_window(&mut self, req: LinkOpts) -> Response {
        let result = self.window_index(req.src).and_then(|src| {
            self.workspace
                .link_window(src, req.dst, req.place)
                .map(|(_, destroyed)| destroyed)
        });
        self.killed_panes(result)
    }

    /// `unlink-window`: removes one index of a window. The window of an index that is its only one
    /// goes only with `kill`.
    fn unlink_window(&mut self, req: UnlinkOpts) -> Response {
        let result = self
            .window_index(req.index)
            .and_then(|number| self.workspace.unlink_window(number, req.kill));
        self.killed_panes(result)
    }

    /// `resize-window`: pins window `index` (the active one when `None`) to a size of its own,
    /// starting from its current size. `width` and `height` set it, `side` moves that border by
    /// `cells` (1 by default), and `fit` takes the largest or smallest attached client.
    fn resize_window(&mut self, req: ResizeOpts) -> Response {
        let number = match self.window_index(req.index) {
            Ok(number) => number,
            Err(e) => return Response::Error(e),
        };
        let WindowResize {
            width,
            height,
            side,
            cells,
            fit,
        } = req.resize;
        if let Some(bad) = [width, height]
            .into_iter()
            .flatten()
            .find(|v| !(1..=MAX_WINDOW_SIZE).contains(v))
        {
            return Response::Error(format!("size {bad} out of range (1 to {MAX_WINDOW_SIZE})"));
        }
        let (mut w, mut h) = self.window_size_at(number);
        w = width.unwrap_or(w);
        h = height.unwrap_or(h);
        let cells = cells.unwrap_or(1);
        match side {
            Some(Side::Left) => w = shrink(w, cells),
            Some(Side::Right) => w = w.saturating_add(cells),
            Some(Side::Up) => h = shrink(h, cells),
            Some(Side::Down) => h = h.saturating_add(cells),
            None => {}
        }
        if let Some(fit) = fit {
            let policy = match fit {
                WindowFit::Largest => WindowSize::Largest,
                WindowFit::Smallest => WindowSize::Smallest,
            };
            let sized = clients::live(&self.filters);
            (w, h) = effective_size(policy, self.config.window.default_size, &sized, (w, h));
        }
        let size = (w.clamp(1, MAX_WINDOW_SIZE), h.clamp(1, MAX_WINDOW_SIZE));
        unit(self.workspace.set_manual_size(Some(number), Some(size)))
    }

    /// The size window `number` is laid out at: its manual size, else the size the clients give.
    fn window_size_at(&self, number: usize) -> (u16, u16) {
        self.workspace
            .manual_size_at(Some(number))
            .unwrap_or_else(|| self.window_size(self.requested))
    }
}
