//! Command execution: maps each `Command` onto daemon requests and TUI state.
use super::app::{App, Flow, Mode};
use super::command::Command;
use super::copy::CopyState;
use super::keymap;
use super::overlay::{Overlay, OverlayCtx, buffer_summary};
use super::panes::{find_directional, neighbor_pane};
use super::prompt::PromptState;
use super::session::neighbor_session;
use super::session::{pick, split_target};
use crate::config::Config;
use crate::ipc;
use crate::layout::Direction;
use crate::protocol::{LayoutPreset, NewWindowOpts, Side, SpawnOpts, SplitOpts, VarKind};
use std::time::{Duration, Instant};

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
    SpreadLayout,
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
            | Command::ReloadConfig
            | Command::DisplayPanes { .. }
            | Command::ClockMode { .. }
            | Command::ShowMessages
            | Command::ListBuffers
            | Command::ChooseBuffer
            | Command::SuspendClient
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

    /// Runs a command line sent by another client (tmux `detach-client`, `switch-client`); scripted,
    /// so it never asks for confirmation.
    pub async fn run_line(&mut self, line: &str) -> Flow {
        match super::command::parse(line) {
            Ok(cmd) => self.run(cmd).await,
            Err(e) => self.say(e),
        }
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
            Command::SpreadLayout => self.simple(Simple::SpreadLayout).await,
            Command::SwapPane { next } => self.swap(next).await,
            Command::SwapPaneMarked { source, target } => self.swap_marked(source, target).await,
            Command::MarkPane { target } => self.mark_pane(target.as_deref()).await,
            Command::ClearMark => self.clear_mark().await,
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
            Command::ReloadConfig => self.reload_config().await,
            Command::Bind { root, key, line } => self.bind_key(root, &key, &line).await,
            Command::RefreshRuntime => {
                self.apply_runtime().await;
                Flow::Continue
            }
            Command::Tmux { words } => self.run_tmux(&words).await,
            Command::Sequence(commands) => Box::pin(self.run_sequence(commands)).await,
            Command::Zoom => self.toggle_zoom(),
            Command::CopyMode => self.enter_copy().await,
            Command::CopyModePageUp => self.enter_copy_page_up().await,
            Command::Detach => Flow::Quit,
            Command::RenameSession { name } => {
                let result = self.client.rename_session(&name).await;
                if result.is_ok() {
                    self.session = name;
                }
                self.settle(result)
            }
            Command::OpenTree => self.open_overlay(Overlay::tree()).await,
            Command::OpenHelp => self.open_help().await,
            Command::OpenPalette { tab } => self.open_palette(tab).await,
            Command::CommandPrompt { initial } => self.open_prompt(&initial),
            Command::Confirm { prompt, then } => self.ask(prompt, *then),
            Command::DisplayPanes { duration_ms } => self.open_panes(duration_ms),
            Command::ClockMode { target } => self.open_clock(target),
            Command::ShowMessages => {
                let lines = self.message_log.clone();
                self.open_overlay(Overlay::text("messages", lines)).await
            }
            Command::ListBuffers => self.list_buffers().await,
            Command::ChooseBuffer => self.open_overlay(Overlay::buffers()).await,
            Command::SuspendClient => self.suspend_client(),
            other => self.say(format!("unhandled command: {other:?}")),
        }
    }

    /// Re-reads the config file. A file that fails to parse, or a keymap that fails to resolve,
    /// leaves everything unchanged. The daemon reloads the settings it owns (resume rules,
    /// restore, history limit and spawn defaults) from the same file.
    async fn reload_config(&mut self) -> Flow {
        let path = self.config_path.clone().or_else(Config::path);
        let config = match Config::load_at(path.as_deref()) {
            Ok(config) => config,
            Err(e) => return self.say(format!("reload-config: {e}")),
        };
        if let Err(e) = keymap::resolve(&config) {
            return self.say(format!("reload-config: {e}"));
        }
        let daemon = self.client.reload_config(path.as_deref()).await;
        self.base_config = config;
        self.apply_runtime().await;
        self.layout.dirty = true;
        match daemon {
            Ok(()) => self.say("config reloaded"),
            Err(e) => self.say(format!("config reloaded in the UI only: {e}")),
        }
    }

    /// Rebuilds the effective config and keymap: the file's config, then the session's
    /// `set-option` values, then its `bind-key` / `unbind-key` changes. A value that does not
    /// apply is reported and skipped.
    pub async fn apply_runtime(&mut self) {
        let mut config = self.base_config.clone();
        let mut problems = Vec::new();
        for (name, value) in self.client.get_vars(VarKind::Option).await.unwrap_or_default() {
            if let Err(e) = config.apply_option(&name, &value) {
                problems.push(format!("{name}: {e}"));
            }
        }
        let mut keymap = match keymap::resolve(&config) {
            Ok(keymap) => keymap,
            Err(e) => {
                problems.push(e);
                config = self.base_config.clone();
                keymap::resolve(&config).unwrap_or_else(|_| self.keymap.clone())
            }
        };
        for (name, line) in self.client.get_vars(VarKind::Binding).await.unwrap_or_default() {
            let (table, key) = name.split_once(':').unwrap_or(("prefix", &name));
            if let Err(e) = keymap.bind(table == "root", key, &line) {
                problems.push(e);
            }
        }
        self.config = config;
        self.keymap = keymap;
        self.layout.dirty = true;
        if let Some(first) = problems.first() {
            self.say(first.clone());
        }
    }

    /// `bind-key` / `unbind-key`: changes this keymap and records the binding in the session, so
    /// terminals attaching later have it too.
    async fn bind_key(&mut self, root: bool, key: &str, line: &str) -> Flow {
        if let Err(e) = self.keymap.bind(root, key, line) {
            return self.say(e);
        }
        let name = format!("{}:{key}", if root { "root" } else { "prefix" });
        let result = self.client.set_var(VarKind::Binding, &name, Some(line)).await;
        self.settle(result)
    }

    /// Any other tmux command, through the shim against this session. Its output is shown on the
    /// status row.
    async fn run_tmux(&mut self, words: &[String]) -> Flow {
        let target = crate::tmux_shim::EnvTarget {
            endpoint: self.endpoint.clone(),
            session: self.session.clone(),
        };
        let line = crate::tmux_shim::join_words(words);
        match crate::tmux_shim::run_in(&target, &line).await {
            Ok(out) => {
                self.apply_runtime().await;
                let mut lines: Vec<String> = out.lines().map(str::to_string).collect();
                if lines.len() > 1 {
                    let title = words.first().cloned().unwrap_or_default();
                    return self.open_overlay(Overlay::text(title, lines)).await;
                }
                match lines.pop() {
                    Some(only) => self.say(only),
                    None => Flow::Continue,
                }
            }
            Err(e) => self.say(e),
        }
    }

    /// Commands one after the other, until one quits.
    async fn run_sequence(&mut self, commands: Vec<Command>) -> Flow {
        for cmd in commands {
            if self.run(cmd).await == Flow::Quit {
                return Flow::Quit;
            }
        }
        Flow::Continue
    }

    async fn open_help(&mut self) -> Flow {
        let entries = self.keymap.help_entries();
        self.open_overlay(Overlay::help(entries)).await
    }

    /// `display-panes`: numbers on the panes until a digit picks one, a key is pressed or the time
    /// runs out (`-d`, else `display-panes-time`). A time of 0 waits for a key.
    fn open_panes(&mut self, duration_ms: Option<u64>) -> Flow {
        if self.panes.is_empty() {
            return Flow::Continue;
        }
        let ms = duration_ms.unwrap_or(self.config.display_panes_time_ms);
        let until = (ms > 0).then(|| Instant::now() + Duration::from_millis(ms));
        self.mode = Mode::Panes { until };
        Flow::Continue
    }

    /// `clock-mode`: the clock fills `target` (a visible pane), else the active pane.
    fn open_clock(&mut self, target: Option<String>) -> Flow {
        let raw = target.unwrap_or_else(|| self.active_pane.clone());
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

    /// `list-buffers`: one line per paste buffer, newest first.
    async fn list_buffers(&mut self) -> Flow {
        match self.client.list_buffers().await {
            Ok(buffers) => {
                let lines = buffers.iter().map(buffer_summary).collect();
                self.open_overlay(Overlay::text("list-buffers", lines))
                    .await
            }
            Err(e) => self.say(e),
        }
    }

    /// `suspend-client`: the event loop stops the process once the command has run.
    fn suspend_client(&mut self) -> Flow {
        if cfg!(unix) {
            self.suspend_requested = true;
            Flow::Continue
        } else {
            self.say("suspend-client is not supported on this platform")
        }
    }

    async fn open_palette(&mut self, tab: Option<crate::tui::command::PaletteTab>) -> Flow {
        let entries = self.keymap.help_entries();
        let view = super::overlay::PaletteView::new(
            tab.unwrap_or_default(),
            entries,
            &self.session,
        );
        self.open_overlay(Overlay::palette(view)).await
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
            Simple::SpreadLayout => self.client.spread_layout().await,
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
        let text = if text.is_empty() {
            match &self.last_copy {
                Some(s) => s.clone(),
                None => match &self.clipboard {
                    super::app::Clipboard::Captured(items) => {
                        items.last().cloned().unwrap_or_default()
                    }
                    super::app::Clipboard::System => {
                        super::render::read_clipboard().unwrap_or_default()
                    }
                },
            }
        } else {
            text
        };
        if text.is_empty() {
            return Flow::Continue;
        }
        let result = pick(&mut temp, &mut self.client)
            .paste(&pane, &text, submit)
            .await;
        self.settle(result)
    }

    async fn resize(&mut self, side: Side, cells: u16) -> Flow {
        let pane = self.active_pane.clone();
        let result = self
            .client
            .resize_split(&pane, side, cells, self.pane_viewport())
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

    async fn swap_marked(&mut self, source: Option<String>, target: Option<String>) -> Flow {
        if source.is_none() && target.is_none() && self.layout.marked_pane_global().is_none() {
            return self.say("no marked pane");
        }
        let result = self
            .client
            .swap_pane_targets(source.as_deref(), target.as_deref())
            .await;
        self.settle(result)
    }

    async fn mark_pane(&mut self, target: Option<&str>) -> Flow {
        let result = self.client.mark_pane(target, false).await;
        self.settle(result)
    }

    async fn clear_mark(&mut self) -> Flow {
        let result = self.client.mark_pane(None, true).await;
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

    async fn enter_copy(&mut self) -> Flow {
        if self.active_pane.is_empty() {
            return self.say("no active pane");
        }
        self.load_scrollback(&self.active_pane.clone()).await;
        let cs = CopyState::enter(self.active_vt().map(|vt| vt.grid()));
        self.parked_copy.remove(&self.active_pane);
        self.mode = Mode::Copy(cs);
        Flow::Continue
    }

    async fn enter_copy_page_up(&mut self) -> Flow {
        self.enter_copy().await;
        let Mode::Copy(cs) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Flow::Continue;
        };
        let page_up = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        );
        self.copy_key(cs, page_up).await
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
