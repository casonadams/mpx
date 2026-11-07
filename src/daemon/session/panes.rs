//! Pane requests on the layout: splitting, focus, zoom, swapping, marks, input switches, joins and
//! layout presets.

use super::Placement;
use super::Session;
use super::answer::{check, node_reply, not_found, unit};
use crate::layout::Direction;
use crate::protocol::{PaneRequest, Response, SplitOpts};

impl Session {
    pub(super) async fn pane_request(&mut self, req: PaneRequest) -> Response {
        match req {
            PaneRequest::Split(opts) => self.split(opts).await,
            PaneRequest::FocusNext => {
                self.workspace.focus_next();
                Response::Ok
            }
            PaneRequest::FocusLast { keep_zoom } => check(
                self.workspace.focus_last_with(keep_zoom),
                "No previous pane",
            ),
            PaneRequest::Focus { name, keep_zoom } => {
                self.target_name(&name).map_or_else(Response::Error, |n| {
                    check(self.workspace.focus_with(&n, keep_zoom), &not_found(&n))
                })
            }
            PaneRequest::Zoom { pane, on } => unit(self.zoom(pane, on)),
            PaneRequest::ResizeSplit {
                name,
                side,
                cells,
                width,
                height,
            } => self.target_name(&name).map_or_else(Response::Error, |n| {
                check(
                    self.workspace.resize_split(&n, side, cells, width, height),
                    "No split border to resize",
                )
            }),
            PaneRequest::SwapPane { a, b, keep_zoom } => self.swap_pane(a, b, keep_zoom),
            PaneRequest::MarkPane { name, clear } => self.mark_pane(name, clear),
            PaneRequest::InputOff { name, off } => self.input_off(name, off),
            PaneRequest::BreakPane { name } => {
                self.target_name(&name).map_or_else(Response::Error, |n| {
                    check(self.workspace.break_pane(&n), "Pane is alone in its window")
                })
            }
            PaneRequest::Join {
                src,
                dst,
                side_by_side,
                detach,
            } => self.join_pane(&src, &dst, side_by_side, detach),
            PaneRequest::SelectLayout { preset } => {
                check(self.workspace.select_layout(preset), "No window")
            }
            PaneRequest::SpreadLayout => check(self.workspace.spread_layout(), "No window"),
            PaneRequest::Synchronize { window, on } => unit(self.workspace.synchronize(window, on)),
        }
    }

    /// `split-window`: starts a pane beside `target`; with `keep_zoom` a zoomed window stays zoomed.
    async fn split(&mut self, opts: SplitOpts) -> Response {
        let SplitOpts {
            target,
            spawn,
            direction,
            percent,
            focus,
            keep_zoom,
        } = opts;
        let target = match target.map(|t| self.target_name(&t)).transpose() {
            Ok(target) => target,
            Err(e) => return Response::Error(e),
        };
        let zoomed = keep_zoom && self.workspace.zoomed_at(target.as_deref());
        let place = Placement::Split {
            target,
            dir: direction,
            percent,
            focus,
        };
        let result = self.add_node(spawn, place).await;
        if let (true, Ok(info)) = (zoomed, &result) {
            self.workspace.zoom_window_of(&info.name);
        }
        node_reply(result)
    }

    /// `join-pane`: moves `src` beside `dst`, leaving its window when `detach` is set.
    fn join_pane(&mut self, src: &str, dst: &str, side_by_side: bool, detach: bool) -> Response {
        let (src, dst) = match (self.target_name(src), self.target_name(dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(e), _) | (_, Err(e)) => return Response::Error(e),
        };
        let dir = if side_by_side {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        unit(self.workspace.join_pane(&src, &dst, dir, detach))
    }

    /// Zooms or unzooms the active window (`on`, else toggled). A `pane` is focused first, which
    /// keeps the zoom; without one the window's focused pane is zoomed.
    fn zoom(&mut self, pane: Option<String>, on: Option<bool>) -> Result<(), String> {
        if let Some(pane) = pane {
            let name = self.target_name(&pane)?;
            self.workspace.focus_with(&name, true);
        }
        if self.workspace.set_zoom(on) {
            Ok(())
        } else {
            Err("No window".to_string())
        }
    }

    /// `swap-pane`: swaps `a` (default: the marked pane, else the active one) with `b` (default:
    /// the active pane, else the marked one).
    fn swap_pane(&mut self, a: Option<String>, b: Option<String>, keep_zoom: bool) -> Response {
        let marked = self.workspace.marked_pane.clone();
        let active = self.active_pane();
        let src = match a {
            Some(s) => self.target_name(&s),
            None => marked
                .clone()
                .or_else(|| active.clone())
                .ok_or_else(|| "No pane to swap".to_string()),
        };
        let src = match src {
            Ok(src) => src,
            Err(e) => return Response::Error(e),
        };
        let dst = match b {
            Some(t) => self.target_name(&t),
            None => Ok(match active {
                Some(act) if act != src => act,
                _ => match marked {
                    Some(m) if m != src => m,
                    _ => src.clone(),
                },
            }),
        };
        let dst = match dst {
            Ok(dst) => dst,
            Err(e) => return Response::Error(e),
        };
        if src == dst {
            return Response::Ok;
        }
        let swapped = self.workspace.swap(&src, &dst);
        if swapped && !keep_zoom {
            self.workspace.unzoom();
        }
        check(swapped, "Cannot swap panes")
    }

    /// `select-pane -d` / `-e`: stops or resumes input to the named pane, or the active pane.
    fn input_off(&mut self, name: Option<String>, off: bool) -> Response {
        self.pane_or_active(name)
            .map_or_else(Response::Error, |target| {
                self.workspace.set_input_off(&target, off);
                Response::Ok
            })
    }

    /// `select-pane -m` / `-M`: marks the named pane, or the active one, or clears the mark.
    fn mark_pane(&mut self, name: Option<String>, clear: bool) -> Response {
        if clear {
            self.workspace.clear_mark_pane();
            return Response::Ok;
        }
        self.pane_or_active(name)
            .map_or_else(Response::Error, |target| {
                self.workspace.mark_pane(&target);
                Response::Ok
            })
    }
}
