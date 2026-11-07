use super::Client;
use crate::protocol::{LayoutPreset, NodeInfo, PaneRequest, Request, Response, Side, SplitOpts};

impl Client {
    pub async fn split(&mut self, opts: SplitOpts) -> Result<NodeInfo, String> {
        request!(self, "Split", Request::Pane(PaneRequest::Split(opts)), Response::Node(info) => *info)
    }

    pub async fn break_pane(&mut self, name: &str) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::BreakPane {
            name: name.to_string(),
        });
        self.call_ok("BreakPane", req).await
    }

    pub async fn next_pane(&mut self) -> Result<(), String> {
        self.call_ok("FocusNext", Request::Pane(PaneRequest::FocusNext))
            .await
    }

    pub async fn focus_last(&mut self) -> Result<(), String> {
        self.focus_last_with(false).await
    }

    /// `keep_zoom` (tmux `last-pane -Z`) leaves a zoomed window zoomed.
    pub async fn focus_last_with(&mut self, keep_zoom: bool) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::FocusLast { keep_zoom });
        self.call_ok("FocusLast", req).await
    }

    pub async fn focus(&mut self, name: &str) -> Result<(), String> {
        self.focus_with(name, false).await
    }

    /// `keep_zoom` (tmux `select-pane -Z`) leaves a zoomed window zoomed.
    pub async fn focus_with(&mut self, name: &str, keep_zoom: bool) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::Focus {
            name: name.to_string(),
            keep_zoom,
        });
        self.call_ok("Focus", req).await
    }

    /// Zooms or unzooms the window of `pane` (the active pane when `None`); `on` None toggles.
    pub async fn zoom(&mut self, pane: Option<&str>, on: Option<bool>) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::Zoom {
            pane: pane.map(str::to_string),
            on,
        });
        self.call_ok("Zoom", req).await
    }

    /// Turns `synchronize-panes` on or off for window `window` (the active window when `None`);
    /// `on` None toggles it.
    pub async fn synchronize(
        &mut self,
        window: Option<usize>,
        on: Option<bool>,
    ) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::Synchronize { window, on });
        self.call_ok("Synchronize", req).await
    }

    pub async fn resize_split(
        &mut self,
        name: &str,
        side: Side,
        cells: u16,
        viewport: (u16, u16),
    ) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::ResizeSplit {
            name: name.to_string(),
            side,
            cells,
            width: viewport.0,
            height: viewport.1,
        });
        self.call_ok("ResizeSplit", req).await
    }

    pub async fn swap_pane(&mut self, a: &str, b: &str) -> Result<(), String> {
        self.swap_pane_with(Some(a), Some(b), false).await
    }

    pub async fn swap_pane_targets(
        &mut self,
        a: Option<&str>,
        b: Option<&str>,
        keep_zoom: bool,
    ) -> Result<(), String> {
        self.swap_pane_with(a, b, keep_zoom).await
    }

    /// `keep_zoom` (tmux `swap-pane -Z`) leaves a zoomed window zoomed.
    pub async fn swap_pane_with(
        &mut self,
        a: Option<&str>,
        b: Option<&str>,
        keep_zoom: bool,
    ) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::SwapPane {
            a: a.map(str::to_string),
            b: b.map(str::to_string),
            keep_zoom,
        });
        self.call_ok("SwapPane", req).await
    }

    pub async fn mark_pane(&mut self, name: Option<&str>, clear: bool) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::MarkPane {
            name: name.map(str::to_string),
            clear,
        });
        self.call_ok("MarkPane", req).await
    }

    /// Turns input to pane `name` (the active pane when `None`) off (`select-pane -d`) or on (`-e`).
    pub async fn input_off(&mut self, name: Option<&str>, off: bool) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::InputOff {
            name: name.map(str::to_string),
            off,
        });
        self.call_ok("InputOff", req).await
    }

    pub async fn spread_layout(&mut self) -> Result<(), String> {
        self.call_ok("SpreadLayout", Request::Pane(PaneRequest::SpreadLayout))
            .await
    }

    pub async fn select_layout(&mut self, preset: Option<LayoutPreset>) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::SelectLayout { preset });
        self.call_ok("SelectLayout", req).await
    }

    /// Moves pane `src` next to `dst`, splitting it side by side or stacked; `detach` keeps the
    /// pane that was active before the join active.
    pub async fn join_pane(
        &mut self,
        src: &str,
        dst: &str,
        side_by_side: bool,
        detach: bool,
    ) -> Result<(), String> {
        let req = Request::Pane(PaneRequest::Join {
            src: src.to_string(),
            dst: dst.to_string(),
            side_by_side,
            detach,
        });
        self.call_ok("JoinPane", req).await
    }
}
