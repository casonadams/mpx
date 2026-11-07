//! What opens over the panes: the pane numbers, the clock, popups and menus.
use crate::daemon::plan::fallback_shell;
use crate::tui::app::{App, Flow, Mode};
use crate::tui::command::PopupCommand;
use crate::tui::frame;
use crate::tui::menu::{MenuItem, MenuState};
use crate::tui::placement::Offset;
use crate::tui::popup::{PopupOpts, PopupState};
use crate::tui::session::split_target;
use std::time::{Duration, Instant};

impl App {
    pub(super) fn popup_command(&mut self, cmd: PopupCommand) -> Flow {
        match cmd {
            PopupCommand::Display(opts) => self.open_popup(opts),
            PopupCommand::Menu { title, x, y, items } => self.open_menu(title, x, y, items),
        }
    }

    /// `display-panes`: numbers on the panes until a digit picks one, a key is pressed or the time
    /// runs out (`-d`, else `display-panes-time`). A time of 0 waits for a key.
    pub(super) fn open_panes(&mut self, duration_ms: Option<u64>) -> Flow {
        if self.panes.is_empty() {
            return Flow::Continue;
        }
        let ms = duration_ms.unwrap_or(self.config.display_panes_time_ms);
        let until = (ms > 0).then(|| Instant::now() + Duration::from_millis(ms));
        self.mode = Mode::Panes { until };
        Flow::Continue
    }

    /// `clock-mode`: the clock fills `target` (a visible pane), else the active pane.
    pub(super) fn open_clock(&mut self, target: Option<String>) -> Flow {
        let raw = self.target_or_active(target);
        let (_, pane) = split_target(&raw);
        if self.panes.iter().any(|(name, ..)| name.as_str() == pane) {
            self.mode = Mode::Clock {
                pane: pane.to_string(),
            };
            Flow::Continue
        } else {
            self.say(format!("no pane {raw}"))
        }
    }

    /// `display-popup`: the command starts on a pty in a box over the panes. Its directory is `-d`,
    /// else the directory of the `-t` pane, else the active pane's.
    fn open_popup(&mut self, mut opts: PopupOpts) -> Flow {
        if opts.cwd.is_none() {
            let target = self.target_or_active(opts.target.clone());
            let pane = split_target(&target).1;
            opts.cwd = self
                .nodes
                .iter()
                .find(|node| node.name == pane)
                .and_then(|node| node.cwd.clone());
        }
        let shell = self
            .config
            .session
            .default_shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| fallback_shell().to_string());
        let screen = (self.viewport.0, self.rows.screen_h);
        match PopupState::open(opts, screen, &shell, &self.config.session.default_terminal) {
            Ok(popup) => {
                self.mode = Mode::Popup(Box::new(popup));
                Flow::Continue
            }
            Err(e) => self.say(e),
        }
    }

    /// `display-menu`: the menu opens over the panes; its title is a format, expanded now.
    fn open_menu(
        &mut self,
        title: Option<String>,
        x: Offset,
        y: Offset,
        items: Vec<MenuItem>,
    ) -> Flow {
        let title = title
            .map(|raw| frame::expand_active(self, &raw))
            .unwrap_or_default();
        let screen = (self.viewport.0, self.rows.screen_h);
        self.mode = Mode::Menu(Box::new(MenuState::new(title, items, x, y, screen)));
        Flow::Continue
    }
}
