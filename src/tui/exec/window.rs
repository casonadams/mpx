//! Window commands: create, kill, rename, select, step, respawn, move, swap, link, unlink, resize
//! and find. The user numbers windows from `base-index`; the daemon numbers them from zero.
use crate::layout::LinkPlace;
use crate::protocol::WindowResize;
use crate::tui::app::{App, Flow};
use crate::tui::command::{PaletteTab, WindowCommand};
use crate::tui::overlay::{Overlay, PaletteView};

impl App {
    pub(super) async fn window_command(&mut self, cmd: WindowCommand) -> Flow {
        match cmd {
            WindowCommand::NewWindow { name, argv, cwd } => self.new_window(name, argv, cwd).await,
            WindowCommand::KillWindow { index } => match self.window_target(index) {
                Ok(index) => {
                    let result = self.client.kill_window(index).await;
                    self.settle(result)
                }
                Err(flow) => flow,
            },
            WindowCommand::RenameWindow { name } => {
                let result = self.client.rename_window(None, &name).await;
                self.settle(result)
            }
            WindowCommand::SelectWindow { index } => match self.window_slot(index) {
                Ok(index) => self.select_window(index).await,
                Err(flow) => flow,
            },
            WindowCommand::SelectWindowNamed { name } => self.select_window_named(&name).await,
            WindowCommand::StepWindow {
                forward,
                alert: true,
            } => self.step_alert(forward).await,
            WindowCommand::StepWindow {
                forward,
                alert: false,
            } => {
                let result = if forward {
                    self.client.next_window().await
                } else {
                    self.client.prev_window().await
                };
                self.settle(result)
            }
            WindowCommand::LastWindow => {
                let result = self.client.last_window().await;
                self.settle(result)
            }
            WindowCommand::RotateWindow { down, keep_zoom } => {
                self.rotate_window(down, keep_zoom).await
            }
            WindowCommand::RespawnWindow {
                target,
                kill,
                cwd,
                command,
            } => match self.window_target(target) {
                Ok(index) => self.respawn_window(index, kill, cwd, command).await,
                Err(flow) => flow,
            },
            WindowCommand::MoveWindow { to } => self.move_window(to).await,
            WindowCommand::RenumberWindows => self.renumber_windows().await,
            WindowCommand::SwapWindow { src, dst } => self.swap_window(src, dst).await,
            WindowCommand::LinkWindow(req) => self.link_window(req.src, req.dst, req.place).await,
            WindowCommand::UnlinkWindow(req) => self.unlink_window(req.index, req.kill).await,
            WindowCommand::ResizeWindow(req) => self.resize_window(req.index, req.resize).await,
            WindowCommand::FindWindow { pattern } => self.find_window(&pattern).await,
        }
    }

    /// The daemon's index of window `number`, or "can't find window: N".
    fn window_slot(&mut self, number: usize) -> Result<usize, Flow> {
        match number.checked_sub(self.config.base_index) {
            Some(index) => Ok(index),
            None => Err(self.say(format!("can't find window: {number}"))),
        }
    }

    /// `number` as the daemon's index; `None` (the active window) stays `None`.
    fn window_target(&mut self, number: Option<usize>) -> Result<Option<usize>, Flow> {
        number.map(|number| self.window_slot(number)).transpose()
    }

    /// As `window_target`, for the commands whose message does not name the window.
    fn daemon_window(&mut self, number: Option<usize>) -> Result<Option<usize>, Flow> {
        match number {
            None => Ok(None),
            Some(number) => match number.checked_sub(self.config.base_index) {
                Some(index) => Ok(Some(index)),
                None => Err(self.say("can't find window")),
            },
        }
    }

    async fn select_window(&mut self, index: usize) -> Flow {
        let result = self.client.select_window(index).await;
        self.settle(result)
    }

    /// `select-window -t name`: selects the window with that name, or reports it missing.
    async fn select_window_named(&mut self, name: &str) -> Flow {
        let found = self
            .layout
            .tabs
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.index);
        match found {
            Some(index) => self.select_window(index).await,
            None => self.say(format!("can't find window: {name}")),
        }
    }

    /// The next (or previous) window with activity or a bell, wrapping around.
    async fn step_alert(&mut self, forward: bool) -> Flow {
        let tabs = &self.layout.tabs;
        let count = tabs.len();
        let current = tabs.iter().position(|t| t.is_active).unwrap_or(0);
        let alert = (1..count)
            .map(|step| {
                if forward {
                    (current + step) % count
                } else {
                    (current + count - step) % count
                }
            })
            .find(|&i| tabs[i].has_activity || tabs[i].has_bell || tabs[i].has_silence)
            .map(|i| tabs[i].index);
        match alert {
            Some(index) => self.select_window(index).await,
            None => self.say(format!(
                "no {} window with an alert",
                if forward { "next" } else { "previous" }
            )),
        }
    }

    /// `respawn-window`: restarts every pane of the window (the active window unless numbered).
    async fn respawn_window(
        &mut self,
        index: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    ) -> Flow {
        let command = (!command.is_empty()).then_some(command);
        let result = self.client.respawn_window(index, kill, cwd, command).await;
        self.settle(result)
    }

    /// `move-window -t index`: moves the active window to `to`.
    async fn move_window(&mut self, to: usize) -> Flow {
        let to = match self.window_slot(to) {
            Ok(to) => to,
            Err(flow) => return flow,
        };
        let Some(from) = self.active_window() else {
            return self.say("no active window");
        };
        let result = self.client.move_window(from, to).await;
        self.settle(result)
    }

    /// `swap-window`: exchanges window `src` (the active one unless given) with `dst`.
    async fn swap_window(&mut self, src: Option<usize>, dst: usize) -> Flow {
        let base = self.config.base_index;
        let from = match src {
            Some(number) => number.checked_sub(base),
            None => self.active_window(),
        };
        let (Some(from), Some(to)) = (from, dst.checked_sub(base)) else {
            return self.say("can't find window");
        };
        let result = self.client.swap_window(from, to).await;
        self.settle(result)
    }

    /// `link-window`: shows window `src` at index `dst` as well.
    async fn link_window(
        &mut self,
        src: Option<usize>,
        dst: Option<usize>,
        place: LinkPlace,
    ) -> Flow {
        let (src, dst) = match (self.daemon_window(src), self.daemon_window(dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(flow), _) | (_, Err(flow)) => return flow,
        };
        let result = self.client.link_window(src, dst, place).await;
        self.settle(result)
    }

    /// `unlink-window`: removes one index of a window, killing it with `kill`.
    async fn unlink_window(&mut self, index: Option<usize>, kill: bool) -> Flow {
        match self.daemon_window(index) {
            Ok(index) => {
                let result = self.client.unlink_window(index, kill).await;
                self.settle(result)
            }
            Err(flow) => flow,
        }
    }

    /// `resize-window`: gives a window a size of its own.
    async fn resize_window(&mut self, index: Option<usize>, resize: WindowResize) -> Flow {
        match self.daemon_window(index) {
            Ok(index) => {
                let result = self.client.resize_window(index, resize).await;
                self.settle(result)
            }
            Err(flow) => flow,
        }
    }

    /// `find-window`: the sessions and windows palette with `pattern` typed into its filter.
    async fn find_window(&mut self, pattern: &str) -> Flow {
        let view = PaletteView::new(
            PaletteTab::Sessions,
            self.keymap.help_entries(),
            &self.session,
        )
        .with_query(pattern);
        self.open_overlay(Overlay::palette(view)).await
    }
}
