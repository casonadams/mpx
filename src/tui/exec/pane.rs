//! Pane commands: focus, kill, mark, join, swap, resize, respawn, break and input switches.
use crate::protocol::Side;
use crate::tui::app::{App, Flow};
use crate::tui::command::PaneCommand;
use crate::tui::panes::{find_directional, neighbor_pane};
use crate::tui::session::{pick, split_target};

impl App {
    pub(super) async fn pane_command(&mut self, cmd: PaneCommand) -> Flow {
        match cmd {
            PaneCommand::SplitWindow {
                side_by_side,
                argv,
                cwd,
                detach,
                keep_zoom,
            } => {
                self.split_window(side_by_side, detach, keep_zoom, argv, cwd)
                    .await
            }
            PaneCommand::KillPane { target } => self.kill_pane(target).await,
            PaneCommand::SelectPane { target, keep_zoom } => {
                self.select_pane(target, keep_zoom).await
            }
            PaneCommand::SelectPaneDir { side, keep_zoom } => {
                self.select_dir(side, keep_zoom).await
            }
            PaneCommand::MarkPane { target } => {
                let result = self.client.mark_pane(target.as_deref(), false).await;
                self.settle(result)
            }
            PaneCommand::ClearMark => {
                let result = self.client.mark_pane(None, true).await;
                self.settle(result)
            }
            PaneCommand::InputOff { target, off } => {
                let result = self.client.input_off(target.as_deref(), off).await;
                self.settle(result)
            }
            PaneCommand::JoinPane(join) => {
                self.join_pane(join.src, join.dst, join.side_by_side, join.detach)
                    .await
            }
            PaneCommand::RespawnPane {
                target,
                kill,
                cwd,
                command,
            } => self.respawn_pane(target, kill, cwd, command).await,
            PaneCommand::NextPane => {
                let result = self.client.next_pane().await;
                self.settle(result)
            }
            PaneCommand::LastPane { keep_zoom } => {
                let result = self.client.focus_last_with(keep_zoom).await;
                self.settle(result)
            }
            PaneCommand::ResizePane { side, cells } => self.resize(side, cells).await,
            PaneCommand::SwapPane { next, keep_zoom } => self.swap(next, keep_zoom).await,
            PaneCommand::SwapPaneMarked {
                source,
                target,
                keep_zoom,
            } => self.swap_marked(source, target, keep_zoom).await,
            PaneCommand::BreakPane => {
                let pane = self.active_pane.clone();
                let result = self.client.break_pane(&pane).await;
                self.settle(result)
            }
            PaneCommand::SelectLayout { preset } => self.select_layout(preset).await,
            PaneCommand::SpreadLayout => self.spread_layout().await,
        }
    }

    /// `select-pane -t`: focuses the pane, switching to its session first when it is elsewhere.
    async fn select_pane(&mut self, target: String, keep_zoom: bool) -> Flow {
        let (session, pane) = split_target(&target);
        let (session, pane) = (session.map(str::to_string), pane.to_string());
        if let Some(other) = session.filter(|s| *s != self.session)
            && let Err(e) = self.switch_session(&other).await
        {
            return self.say(e);
        }
        let result = self.client.focus_with(&pane, keep_zoom).await;
        self.settle(result)
    }

    /// `select-pane -L/-R/-U/-D`: focuses the pane next to the active one in that direction.
    async fn select_dir(&mut self, side: Side, keep_zoom: bool) -> Flow {
        match find_directional(&self.panes, &self.active_pane, side) {
            Some(pane) => {
                let result = self.client.focus_with(&pane, keep_zoom).await;
                self.settle(result)
            }
            None => Flow::Continue,
        }
    }

    async fn kill_pane(&mut self, target: Option<String>) -> Flow {
        let (mut temp, pane) = match self.target_pane(target.as_deref()).await {
            Ok(found) => found,
            Err(flow) => return flow,
        };
        let result = pick(&mut temp, &mut self.client).kill(&pane).await;
        self.settle(result)
    }

    /// `respawn-pane`: restarts the pane's process (the active pane unless named).
    async fn respawn_pane(
        &mut self,
        target: Option<String>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    ) -> Flow {
        let name = self.target_or_active(target);
        let command = (!command.is_empty()).then_some(command);
        let result = self.client.respawn(&name, kill, cwd, command).await;
        self.settle(result)
    }

    /// `join-pane`: moves `src` (the marked pane unless named) next to `dst` (the active pane
    /// unless named). `detach` keeps the pane that was active before the join active.
    async fn join_pane(
        &mut self,
        src: Option<String>,
        dst: Option<String>,
        side_by_side: bool,
        detach: bool,
    ) -> Flow {
        let Some(src) = src.or_else(|| self.layout.marked_pane_global().map(str::to_string)) else {
            return self.say("no marked pane");
        };
        let dst = self.target_or_active(dst);
        let result = self
            .client
            .join_pane(&src, &dst, side_by_side, detach)
            .await;
        self.settle(result)
    }

    /// `resize-pane -L/-R/-U/-D`: moves the active pane's edge by `cells`.
    async fn resize(&mut self, side: Side, cells: u16) -> Flow {
        let pane = self.active_pane.clone();
        let result = self
            .client
            .resize_split(&pane, side, cells, self.pane_viewport())
            .await;
        self.settle(result)
    }

    /// `swap-pane -U/-D`: exchanges the active pane with its neighbour in the layout.
    async fn swap(&mut self, next: bool, keep_zoom: bool) -> Flow {
        let Some(other) = neighbor_pane(&self.layout.raw, &self.active_pane, next) else {
            return Flow::Continue;
        };
        let active = self.active_pane.clone();
        let result = self
            .client
            .swap_pane_with(Some(active.as_str()), Some(other.as_str()), keep_zoom)
            .await;
        self.settle(result)
    }

    async fn swap_marked(
        &mut self,
        source: Option<String>,
        target: Option<String>,
        keep_zoom: bool,
    ) -> Flow {
        if source.is_none() && target.is_none() && self.layout.marked_pane_global().is_none() {
            return self.say("no marked pane");
        }
        let result = self
            .client
            .swap_pane_targets(source.as_deref(), target.as_deref(), keep_zoom)
            .await;
        self.settle(result)
    }
}
