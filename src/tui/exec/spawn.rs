//! Creating panes and windows: the spawn options of a new process and its working directory.
use crate::layout::Direction;
use crate::protocol::{NewWindowOpts, SpawnOpts, SplitOpts};
use crate::tui::app::{App, Flow};

impl App {
    /// `split-window`: a new pane beside the active one (`side_by_side` for a vertical divider).
    pub(super) async fn split_window(
        &mut self,
        side_by_side: bool,
        detach: bool,
        keep_zoom: bool,
        argv: Vec<String>,
        cwd: Option<String>,
    ) -> Flow {
        let name = self.fresh_name("pane");
        let spawn = self.spawn_opts(name, argv, cwd).await;
        let opts = SplitOpts {
            target: self.active_target(),
            spawn,
            direction: if side_by_side {
                Direction::Horizontal
            } else {
                Direction::Vertical
            },
            percent: None,
            focus: !detach,
            keep_zoom,
        };
        let result = self.client.split(opts).await;
        self.settle(result)
    }

    /// `new-window`: a window with one pane, which takes the focus.
    pub(super) async fn new_window(
        &mut self,
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
    ) -> Flow {
        let node_name = name.clone().unwrap_or_default();
        let spawn = self.spawn_opts(node_name, argv, cwd).await;
        let result = self
            .client
            .new_window(NewWindowOpts {
                name,
                spawn,
                focus: true,
            })
            .await;
        self.settle(result)
    }

    /// Working directory of the active pane, falling back to the TUI's own.
    pub(super) async fn active_cwd(&mut self) -> Option<String> {
        let from_pane = if self.active_pane.is_empty() {
            None
        } else {
            let pane = self.active_pane.clone();
            self.client.get_cwd(&pane).await.ok().flatten()
        };
        from_pane.or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
    }

    /// Spawn options for a new pane: the given argv in the given or active cwd. With no argv the
    /// daemon starts the session's default command (`default-command`, else `default-shell`).
    async fn spawn_opts(
        &mut self,
        name: String,
        argv: Vec<String>,
        cwd: Option<String>,
    ) -> SpawnOpts {
        let cwd = match cwd {
            Some(dir) => Some(dir),
            None => self.active_cwd().await,
        };
        let (command, args) = match argv.split_first() {
            Some((command, rest)) => (command.clone(), rest.to_vec()),
            None => (String::new(), Vec::new()),
        };
        SpawnOpts {
            name,
            command,
            args,
            pty: true,
            cwd,
            ..SpawnOpts::default()
        }
    }

    fn active_target(&self) -> Option<String> {
        (!self.active_pane.is_empty()).then(|| self.active_pane.clone())
    }

    /// `prefix-N` with the next counter value not used by any node in this session.
    fn fresh_name(&mut self, prefix: &str) -> String {
        loop {
            self.split_counter += 1;
            let name = format!("{prefix}-{}", self.split_counter);
            if !self.nodes.iter().any(|n| n.name == name) {
                return name;
            }
        }
    }
}
