use super::Client;
use crate::layout::LinkPlace;
use crate::protocol::{
    LinkOpts, NewWindowOpts, NodeInfo, Request, ResizeOpts, Response, UnlinkOpts, WindowRequest,
    WindowResize,
};

impl Client {
    pub async fn new_window(&mut self, opts: NewWindowOpts) -> Result<NodeInfo, String> {
        request!(self, "NewWindow", Request::Window(WindowRequest::New(opts)), Response::Node(info) => *info)
    }

    pub async fn next_window(&mut self) -> Result<(), String> {
        self.call_ok("NextWindow", Request::Window(WindowRequest::Next))
            .await
    }

    pub async fn prev_window(&mut self) -> Result<(), String> {
        self.call_ok("PrevWindow", Request::Window(WindowRequest::Prev))
            .await
    }

    pub async fn last_window(&mut self) -> Result<(), String> {
        self.call_ok("LastWindow", Request::Window(WindowRequest::Last))
            .await
    }

    pub async fn select_window(&mut self, index: usize) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Select { index });
        self.call_ok("SelectWindow", req).await
    }

    pub async fn rename_window(&mut self, index: Option<usize>, name: &str) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Rename {
            index,
            name: name.to_string(),
        });
        self.call_ok("RenameWindow", req).await
    }

    pub async fn kill_window(&mut self, index: Option<usize>) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Kill { index });
        self.call_ok("KillWindow", req).await
    }

    /// Rotates the active window's panes through its layout slots (`rotate-window`); `keep_zoom`
    /// (`-Z`) keeps a zoomed window zoomed.
    pub async fn rotate_window(&mut self, down: bool, keep_zoom: bool) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Rotate { down, keep_zoom });
        self.call_ok("RotateWindow", req).await
    }

    pub async fn move_window(&mut self, from: usize, to: usize) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Move { from, to });
        self.call_ok("MoveWindow", req).await
    }

    /// Renumbers the windows 0, 1, ... in order (tmux `move-window -r`).
    pub async fn renumber_windows(&mut self) -> Result<(), String> {
        self.call_ok("RenumberWindows", Request::Window(WindowRequest::Renumber))
            .await
    }

    pub async fn swap_window(&mut self, a: usize, b: usize) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Swap { a, b });
        self.call_ok("SwapWindow", req).await
    }

    /// Links window `src` at index `dst` as well (`link-window`); see `WindowRequest::LinkWindow`.
    pub async fn link_window(
        &mut self,
        src: Option<usize>,
        dst: Option<usize>,
        place: LinkPlace,
    ) -> Result<(), String> {
        let req = Request::Window(WindowRequest::LinkWindow(LinkOpts { src, dst, place }));
        self.call_ok("LinkWindow", req).await
    }

    /// Removes index `index` of its window (`unlink-window`); see `WindowRequest::UnlinkWindow`.
    pub async fn unlink_window(&mut self, index: Option<usize>, kill: bool) -> Result<(), String> {
        let req = Request::Window(WindowRequest::UnlinkWindow(UnlinkOpts { index, kill }));
        self.call_ok("UnlinkWindow", req).await
    }

    /// Sizes window `index` by hand (`resize-window`); see `WindowRequest::ResizeWindow`.
    pub async fn resize_window(
        &mut self,
        index: Option<usize>,
        resize: WindowResize,
    ) -> Result<(), String> {
        let req = Request::Window(WindowRequest::ResizeWindow(ResizeOpts { index, resize }));
        self.call_ok("ResizeWindow", req).await
    }

    /// Restarts every pane of a window (`respawn-window`); see `Request::Window(WindowRequest::Respawn)`.
    pub async fn respawn_window(
        &mut self,
        index: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<(), String> {
        let req = Request::Window(WindowRequest::Respawn {
            index,
            kill,
            cwd,
            command,
        });
        self.call_ok("RespawnWindow", req).await
    }
}
