//! Command execution: maps each `Command` onto daemon requests and TUI state.
use super::app::{App, Flow, Mode};
use super::command::Command;
use super::copy::CopyState;
use super::overlay::{Overlay, OverlayCtx};
use super::panes::{find_directional, neighbor_pane};
use super::prompt::PromptState;
use super::session::neighbor_session;
use super::session::{pick, split_target};
use crate::ipc;
use crate::layout::Direction;
use crate::protocol::{LayoutPreset, NewWindowOpts, Side, SpawnOpts, SplitOpts};

/// Client calls that need only the active window.
enum Simple {
    NextWindow,
    PrevWindow,
    LastWindow,
    SelectWindow(usize),
    KillWindow(Option<usize>),
    RenameWindow(String),
    FocusNext,
    FocusLast,
    BreakPane,
    SelectLayout(Option<LayoutPreset>),
}

pub fn default_shell() -> String {
    match std::env::var("SHELL") {
        Ok(sh) if !sh.is_empty() => sh,
        _ if cfg!(windows) => "powershell.exe".to_string(),
        _ => "/bin/sh".to_string(),
    }
}

/// Commands that change which pane or window is shown; they drop zoom.
fn clears_zoom(cmd: &Command) -> bool {
    !matches!(
        cmd,
        Command::Zoom
            | Command::CopyMode
            | Command::OpenAgents
            | Command::OpenTree
            | Command::OpenHelp
            | Command::CommandPrompt { .. }
            | Command::Confirm { .. }
            | Command::SendKeys { .. }
            | Command::Paste { .. }
            | Command::ResizePane { .. }
            | Command::Detach
            | Command::KillServer
            | Command::SendPrefix
    )
}

/// Commands that destroy something interactively need a `y` first.
fn needs_confirm(cmd: &Command) -> Option<String> {
    match cmd {
        Command::KillWindow { index: None } => Some("kill-window? (y/n)".to_string()),
        _ => None,
    }
}

impl App {
    /// Runs a command typed or bound by the user, asking first when it is destructive.
    pub async fn dispatch(&mut self, cmd: Command) -> Flow {
        self.layout.dirty = true;
        if let Some(question) = needs_confirm(&cmd) {
            self.mode = Mode::Confirm(question, cmd);
            return Flow::Continue;
        }
        if let Command::KillPane { target: None } = cmd {
            self.mode = Mode::Confirm(format!("kill-pane {}? (y/n)", self.active_pane), cmd);
            return Flow::Continue;
        }
        self.run(cmd).await
    }

    /// Runs a command without the interactive confirmation step (used once confirmed).
    pub async fn run(&mut self, cmd: Command) -> Flow {
        if clears_zoom(&cmd) {
            self.zoomed = None;
        }
        let cmd = match self.run_windows(cmd).await {
            Ok(flow) => return flow,
            Err(cmd) => cmd,
        };
        match self.run_panes(cmd).await {
            Ok(flow) => flow,
            Err(cmd) => self.run_ui(cmd).await,
        }
    }

    /// Window and layout operations. Other commands come back as `Err`.
    async fn run_windows(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::SplitWindow {
                side_by_side,
                argv,
                cwd,
                detach,
            } => self.split_window(side_by_side, argv, cwd, detach).await,
            Command::NewWindow { name, argv, cwd } => self.new_window(name, argv, cwd).await,
            Command::KillPane { target } => self.kill_pane(target).await,
            Command::KillWindow { index } => self.simple(Simple::KillWindow(index)).await,
            Command::RenameWindow { name } => self.simple(Simple::RenameWindow(name)).await,
            Command::SelectWindow { index } => self.simple(Simple::SelectWindow(index)).await,
            Command::NextWindow => self.simple(Simple::NextWindow).await,
            Command::PrevWindow => self.simple(Simple::PrevWindow).await,
            Command::LastWindow => self.simple(Simple::LastWindow).await,
            Command::BreakPane => self.simple(Simple::BreakPane).await,
            Command::SelectLayout { preset } => self.simple(Simple::SelectLayout(preset)).await,
            Command::SwapPane { next } => self.swap(next).await,
            other => return Err(other),
        };
        Ok(flow)
    }

    /// Pane focus, keys, resize and agent operations. Other commands come back as `Err`.
    async fn run_panes(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::SelectPane { target } => self.select_pane(target).await,
            Command::SelectPaneDir { side } => self.select_dir(side).await,
            Command::NextPane => self.simple(Simple::FocusNext).await,
            Command::LastPane => self.simple(Simple::FocusLast).await,
            Command::ResizePane { side, cells } => self.resize(side, cells).await,
            Command::SendKeys { target, keys } => self.send_keys(target, keys).await,
            Command::Paste {
                target,
                text,
                submit,
            } => self.paste(target, text, submit).await,
            Command::SendPrefix => self.send_prefix().await,
            Command::NextAttention => self.next_attention().await,
            Command::LaunchAgent {
                harness,
                resume,
                cwd,
                split,
            } => self.launch(harness, resume, cwd, split).await,
            other => return Err(other),
        };
        Ok(flow)
    }

    /// Sessions, modes, overlays and the remaining commands.
    async fn run_ui(&mut self, cmd: Command) -> Flow {
        match cmd {
            Command::SwitchSession { name } => self.go_session(&name).await,
            Command::NextSession => self.step_session(true).await,
            Command::PrevSession => self.step_session(false).await,
            Command::KillServer => self.kill_server().await,
            Command::Zoom => self.toggle_zoom(),
            Command::CopyMode => self.enter_copy(),
            Command::Detach => Flow::Quit,
            Command::OpenAgents => self.open_overlay(Overlay::agents()).await,
            Command::OpenTree => self.open_overlay(Overlay::tree()).await,
            Command::OpenHelp => self.open_help().await,
            Command::CommandPrompt { initial } => self.open_prompt(&initial),
            Command::Confirm { prompt, then } => self.ask(prompt, *then),
            other => self.say(format!("unhandled command: {other:?}")),
        }
    }

    async fn open_help(&mut self) -> Flow {
        let entries = self.keymap.help_entries();
        self.open_overlay(Overlay::help(entries)).await
    }

    fn open_prompt(&mut self, initial: &str) -> Flow {
        self.mode = Mode::Prompt(PromptState::new(initial));
        Flow::Continue
    }

    fn ask(&mut self, prompt: String, then: Command) -> Flow {
        self.mode = Mode::Confirm(prompt, then);
        Flow::Continue
    }

    /// Reports a failed client call on the status row; success leaves no message.
    pub fn settle<T>(&mut self, result: Result<T, String>) -> Flow {
        match result {
            Ok(_) => Flow::Continue,
            Err(e) => self.say(e),
        }
    }

    async fn simple(&mut self, op: Simple) -> Flow {
        let result = match op {
            Simple::NextWindow => {
                self.zoomed = None;
                self.client.next_window().await
            }
            Simple::PrevWindow => self.client.prev_window().await,
            Simple::LastWindow => self.client.last_window().await,
            Simple::SelectWindow(index) => self.client.select_window(index).await,
            Simple::KillWindow(index) => self.client.kill_window(index).await,
            Simple::RenameWindow(name) => self.client.rename_window(None, &name).await,
            Simple::FocusNext => self.client.next_pane().await,
            Simple::FocusLast => self.client.focus_last().await,
            Simple::BreakPane => {
                let pane = self.active_pane.clone();
                self.client.break_pane(&pane).await
            }
            Simple::SelectLayout(preset) => self.client.select_layout(preset).await,
        };
        self.settle(result)
    }

    async fn step_session(&mut self, next: bool) -> Flow {
        let names: Vec<String> = ipc::list_sessions(&self.socket_dir)
            .into_iter()
            .map(|s| s.name)
            .collect();
        match neighbor_session(&names, &self.session, next) {
            Some(name) => self.go_session(&name).await,
            None => self.say("no other session"),
        }
    }

    async fn select_pane(&mut self, target: String) -> Flow {
        let (session, pane) = split_target(&target);
        let (session, pane) = (session.map(str::to_string), pane.to_string());
        if let Some(other) = session.filter(|s| *s != self.session)
            && let Err(e) = self.switch_session(&other).await
        {
            return self.say(e);
        }
        let result = self.client.focus(&pane).await;
        self.settle(result)
    }

    async fn select_dir(&mut self, side: Side) -> Flow {
        match find_directional(&self.panes, &self.active_pane, side) {
            Some(pane) => {
                let result = self.client.focus(&pane).await;
                self.settle(result)
            }
            None => Flow::Continue,
        }
    }

    async fn split_window(
        &mut self,
        side_by_side: bool,
        argv: Vec<String>,
        cwd: Option<String>,
        detach: bool,
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
        };
        let result = self.client.split(opts).await;
        self.settle(result)
    }

    async fn new_window(
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

    async fn kill_pane(&mut self, target: Option<String>) -> Flow {
        let (mut temp, pane) = match self.resolve_target(target.as_deref()).await {
            Ok(found) => found,
            Err(e) => return self.say(e),
        };
        let result = pick(&mut temp, &mut self.client).kill(&pane).await;
        self.settle(result)
    }

    async fn send_keys(&mut self, target: Option<String>, keys: Vec<String>) -> Flow {
        let (mut temp, pane) = match self.resolve_target(target.as_deref()).await {
            Ok(found) => found,
            Err(e) => return self.say(e),
        };
        let result = pick(&mut temp, &mut self.client)
            .send_keys(&pane, keys, false)
            .await;
        self.settle(result)
    }

    async fn paste(&mut self, target: Option<String>, text: String, submit: bool) -> Flow {
        let (mut temp, pane) = match self.resolve_target(target.as_deref()).await {
            Ok(found) => found,
            Err(e) => return self.say(e),
        };
        let result = pick(&mut temp, &mut self.client)
            .paste(&pane, &text, submit)
            .await;
        self.settle(result)
    }

    async fn resize(&mut self, side: Side, cells: u16) -> Flow {
        let pane = self.active_pane.clone();
        let result = self
            .client
            .resize_split(&pane, side, cells, self.viewport)
            .await;
        self.settle(result)
    }

    async fn swap(&mut self, next: bool) -> Flow {
        let Some(other) = neighbor_pane(&self.layout.raw, &self.active_pane, next) else {
            return Flow::Continue;
        };
        let active = self.active_pane.clone();
        let result = self.client.swap_pane(&active, &other).await;
        self.settle(result)
    }

    fn toggle_zoom(&mut self) -> Flow {
        if self.zoomed.is_some() {
            self.zoomed = None;
        } else if !self.active_pane.is_empty() {
            self.zoomed = Some(self.active_pane.clone());
        }
        Flow::Continue
    }

    fn enter_copy(&mut self) -> Flow {
        if self.active_pane.is_empty() {
            return self.say("no active pane");
        }
        let cs = CopyState::enter(self.active_vt().map(|vt| vt.grid()));
        self.parked_copy.remove(&self.active_pane);
        self.mode = Mode::Copy(cs);
        Flow::Continue
    }

    async fn kill_server(&mut self) -> Flow {
        let _ = self.client.shutdown().await;
        Flow::Quit
    }

    async fn send_prefix(&mut self) -> Flow {
        let modes = self.modes_of(&self.active_pane.clone());
        let bytes = self.keymap.prefix().encode(&modes);
        let result = self.write_active(&bytes).await;
        self.settle(result)
    }

    /// Shows an overlay and loads its data.
    pub async fn open_overlay(&mut self, overlay: Overlay) -> Flow {
        self.mode = Mode::Overlay(overlay);
        self.refresh_overlay().await;
        Flow::Continue
    }

    pub async fn refresh_overlay(&mut self) {
        let Mode::Overlay(mut overlay) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        let cwd = self.active_cwd().await;
        let mut ctx = OverlayCtx {
            client: &mut self.client,
            session: &self.session,
            config: &self.config,
            registry: &self.registry,
            cwd: cwd.as_deref(),
            socket_dir: &self.socket_dir,
        };
        overlay.refresh(&mut ctx).await;
        self.mode = Mode::Overlay(overlay);
    }

    /// Working directory of the active pane, falling back to the TUI's own.
    pub async fn active_cwd(&mut self) -> Option<String> {
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

    /// Spawn options for a new pane: the given argv (or the default shell) in the given or active cwd.
    pub async fn spawn_opts(
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
            None => (default_shell(), Vec::new()),
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
    pub fn fresh_name(&mut self, prefix: &str) -> String {
        loop {
            self.split_counter += 1;
            let name = format!("{prefix}-{}", self.split_counter);
            if !self.nodes.iter().any(|n| n.name == name) {
                return name;
            }
        }
    }
}
