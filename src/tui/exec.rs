//! Command execution: maps each `Command` onto daemon requests and TUI state.
use super::app::{App, Flow, Mode};
use super::command::{
    BufferCommand, ClientCommand, Command, CopyModeOpts, EnvCommand, PaneCommand, PopupCommand,
    PromptSpec, ScriptCommand, SessionCommand, WindowCommand, parse_in,
};
use super::copy::{CopyAction, CopyOutcome, CopyState, Motion, apply_action};
use super::frame;
use super::job::{self, Job, Work};
use super::keymap;
use super::menu::{MenuItem, MenuState};
use super::overlay::{Overlay, OverlayCtx, buffer_summary};
use super::panes::{find_directional, neighbor_pane};
use super::placement::Offset;
use super::popup::{PopupOpts, PopupState};
use super::prompt::{PromptRun, Question};
use super::session::neighbor_session;
use super::session::{pick, split_target};
use crate::config::Config;
use crate::format::{buffer_vars, expand};
use crate::ipc;
use crate::layout::Direction;
use crate::protocol::{
    LayoutPreset, NewWindowOpts, PasteBufferOpts, Request, Response, ScriptRequest, Side,
    SpawnOpts, SplitOpts,
};
use crate::vt::capture::CaptureSpec;
use std::collections::VecDeque;
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// What one command of a sequence does: nothing more, a branch to run next, a job to start, or
/// a quit.
enum Step {
    Next,
    Branch(Command),
    /// A job that the commands after it do not wait for.
    Background(Work),
    /// A job that the commands after it wait for.
    Wait(Work),
    Quit,
}

/// Client calls that need only the active window.
enum Simple {
    NextWindow,
    PrevWindow,
    LastWindow,
    SelectWindow(usize),
    KillWindow(Option<usize>),
    RenameWindow(String),
    FocusNext,
    FocusLast(bool),
    BreakPane,
    SelectLayout(Option<LayoutPreset>),
    SpreadLayout,
}

/// The flags of `split-window`: the direction, `-d` (keep the focus) and `-Z` (keep the zoom).
#[derive(Clone, Copy)]
struct SplitFlags {
    side_by_side: bool,
    detach: bool,
    keep_zoom: bool,
}

/// What `display-message` shows without a format.
const DEFAULT_DISPLAY_MESSAGE: &str = "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index} - (#{pane_title})";

/// Commands that destroy something interactively need a `y` first.
fn needs_confirm(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Window(WindowCommand::KillWindow { index: None }) => {
            Some("kill-window? (y/n)".to_string())
        }
        _ => None,
    }
}

/// Pipes `text` to the shell `command` (`copy-pipe`). Its output is discarded and the TUI does not
/// wait for it: the text is written and the child reaped on a thread of its own.
fn pipe_text(command: &str, text: &str) {
    let spawned = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else { return };
    let text = text.to_string();
    std::thread::spawn(move || {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}

impl App {
    /// Runs a command typed or bound by the user, asking first when it is destructive.
    pub async fn dispatch(&mut self, cmd: Command) -> Flow {
        self.layout.dirty = true;
        if let Some(question) = needs_confirm(&cmd) {
            self.mode = Mode::Confirm(question, cmd);
            return Flow::Continue;
        }
        if let Command::Pane(PaneCommand::KillPane { target: None }) = cmd {
            self.mode = Mode::Confirm(format!("kill-pane {}? (y/n)", self.active_pane), cmd);
            return Flow::Continue;
        }
        self.run(cmd).await
    }

    /// Runs a command without the interactive confirmation step (used once confirmed).
    pub async fn run(&mut self, cmd: Command) -> Flow {
        if self.attachment.terminal.readonly && !super::command::read_only_allowed(&cmd) {
            return self.say("read-only client: this command would change the session");
        }
        let cmd = match self.run_windows(cmd).await {
            Ok(flow) => return flow,
            Err(cmd) => cmd,
        };
        match self.run_pane(cmd).await {
            Ok(flow) => flow,
            Err(cmd) => self.run_ui(cmd).await,
        }
    }

    /// Window and layout operations. Other commands come back as `Err`.
    async fn run_windows(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::Pane(PaneCommand::SplitWindow {
                side_by_side,
                argv,
                cwd,
                detach,
                keep_zoom,
            }) => {
                let flags = SplitFlags {
                    side_by_side,
                    detach,
                    keep_zoom,
                };
                self.split_window(flags, argv, cwd).await
            }
            Command::Window(WindowCommand::NewWindow { name, argv, cwd }) => {
                self.new_window(name, argv, cwd).await
            }
            Command::Pane(PaneCommand::KillPane { target }) => self.kill_pane(target).await,
            Command::Window(WindowCommand::KillWindow { index }) => match index
                .map(|i| i.checked_sub(self.config.base_index))
            {
                None => self.simple(Simple::KillWindow(None)).await,
                Some(Some(daemon)) => self.simple(Simple::KillWindow(Some(daemon))).await,
                Some(None) => self.say(format!("can't find window: {}", index.unwrap_or_default())),
            },
            Command::Window(WindowCommand::RenameWindow { name }) => {
                self.simple(Simple::RenameWindow(name)).await
            }
            Command::Window(WindowCommand::SelectWindow { index }) => {
                match index.checked_sub(self.config.base_index) {
                    Some(daemon) => self.simple(Simple::SelectWindow(daemon)).await,
                    None => self.say(format!("can't find window: {index}")),
                }
            }
            Command::Window(WindowCommand::SelectWindowNamed { name }) => {
                self.select_window_named(name).await
            }
            Command::Window(WindowCommand::StepAlert { forward }) => self.step_alert(forward).await,
            Command::DisplayMessage { format } => {
                let format = format.as_deref().unwrap_or(DEFAULT_DISPLAY_MESSAGE);
                let text = super::frame::expand_active(self, format);
                self.say(text)
            }
            other => return self.run_layout(other).await,
        };
        Ok(flow)
    }

    /// Window layout operations split from `run_windows`: rotation, respawn, moves and renumbering.
    /// Other commands come back as `Err`.
    async fn run_layout(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::Window(WindowCommand::RotateWindow { down, keep_zoom }) => {
                let result = self.client.rotate_window(down, keep_zoom).await;
                self.settle(result)
            }
            Command::Window(WindowCommand::RespawnWindow {
                target,
                kill,
                cwd,
                command,
            }) => self.respawn_window(target, kill, cwd, command).await,
            Command::Window(WindowCommand::MoveWindow { to }) => self.move_window(to).await,
            Command::Window(WindowCommand::RenumberWindows) => {
                let result = self.client.renumber_windows().await;
                self.settle(result)
            }
            other => return Err(other),
        };
        Ok(flow)
    }

    /// Pane, window-navigation and layout operations. Other commands come back as `Err`.
    async fn run_pane(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::Window(WindowCommand::NextWindow) => self.simple(Simple::NextWindow).await,
            Command::Window(WindowCommand::PrevWindow) => self.simple(Simple::PrevWindow).await,
            Command::Window(WindowCommand::LastWindow) => self.simple(Simple::LastWindow).await,
            Command::Pane(PaneCommand::BreakPane) => self.simple(Simple::BreakPane).await,
            Command::Pane(PaneCommand::SelectLayout { preset }) => {
                self.simple(Simple::SelectLayout(preset)).await
            }
            Command::Pane(PaneCommand::SpreadLayout) => self.simple(Simple::SpreadLayout).await,
            Command::Pane(PaneCommand::SwapPane { next, keep_zoom }) => {
                self.swap(next, keep_zoom).await
            }
            Command::Pane(PaneCommand::SwapPaneMarked {
                source,
                target,
                keep_zoom,
            }) => self.swap_marked(source, target, keep_zoom).await,
            Command::Pane(PaneCommand::MarkPane { target }) => {
                self.mark_pane(target.as_deref()).await
            }
            Command::Pane(PaneCommand::ClearMark) => self.clear_mark().await,
            Command::Pane(PaneCommand::InputOff { target, off }) => self.input_off(target, off).await,
            Command::Pane(PaneCommand::SelectPane { target, keep_zoom }) => {
                self.select_pane(target, keep_zoom).await
            }
            Command::Pane(PaneCommand::SelectPaneDir { side, keep_zoom }) => {
                self.select_dir(side, keep_zoom).await
            }
            Command::Pane(PaneCommand::NextPane) => self.simple(Simple::FocusNext).await,
            Command::Pane(PaneCommand::LastPane { keep_zoom }) => {
                self.simple(Simple::FocusLast(keep_zoom)).await
            }
            Command::Pane(PaneCommand::ResizePane { side, cells }) => {
                self.resize(side, cells).await
            }
            Command::SendKeys { target, keys } => self.send_keys(target, keys).await,
            Command::CopyCommand {
                target,
                action,
                count,
            } => self.copy_command(target, action, count).await,
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
            Command::Session(SessionCommand::SwitchSession { name }) => {
                self.go_session(&name).await
            }
            Command::Session(SessionCommand::NextSession) => self.step_session(true).await,
            Command::Session(SessionCommand::PrevSession) => self.step_session(false).await,
            Command::Session(SessionCommand::LastSession) => self.last_session().await,
            Command::KillServer => self.kill_server().await,
            Command::ReloadConfig => self.reload_config().await,
            Command::SetOption {
                name,
                value,
                unset,
                quiet,
            } => self.set_option(&name, value.as_deref(), unset, quiet).await,
            Command::ShowOptions {
                name,
                value_only,
                quiet,
            } => self.show_options(name.as_deref(), value_only, quiet).await,
            Command::Pane(PaneCommand::JoinPane {
                src,
                dst,
                side_by_side,
                detach,
            }) => self.join_pane(src, dst, side_by_side, detach).await,
            Command::Window(WindowCommand::SwapWindow { src, dst }) => {
                self.swap_window(src, dst).await
            }
            Command::Pane(PaneCommand::RespawnPane {
                target,
                kill,
                cwd,
                command,
            }) => self.respawn_pane(target, kill, cwd, command).await,
            Command::Bind { table, key, line } => self.bind_key(table, &key, &line).await,
            Command::Window(WindowCommand::FindWindow { pattern }) => {
                self.find_window(&pattern).await
            }
            Command::Sequence(commands) => Box::pin(self.run_sequence(commands)).await,
            Command::Zoom => self.toggle_zoom().await,
            Command::SetSynchronize { all, on } => self.set_synchronize(all, on).await,
            Command::CopyMode(opts) => self.copy_mode_cmd(opts).await,
            Command::Detach => Flow::Quit,
            Command::Session(SessionCommand::RenameSession { name }) => {
                self.rename_session_cmd(name).await
            }
            Command::Buffer(cmd) => self.buffer_command(cmd).await,
            Command::Environment(cmd) => self.environment_command(cmd).await,
            Command::Client(cmd) => self.client_command(cmd).await,
            Command::Script(script) => self.run_script(script).await,
            other => match self.run_overlay(other).await {
                Ok(flow) => flow,
                Err(cmd) => self.say(format!("unhandled command: {cmd:?}")),
            },
        }
    }

    /// Switches to the session that was current before this one, if there was one.
    async fn last_session(&mut self) -> Flow {
        match self.attachment.previous_session.clone() {
            Some(name) => self.go_session(&name).await,
            None => self.say("no last session"),
        }
    }

    /// Overlays, prompts and the message log. Other commands come back as `Err`.
    async fn run_overlay(&mut self, cmd: Command) -> Result<Flow, Command> {
        let flow = match cmd {
            Command::OpenTree => self.open_overlay(Overlay::tree()).await,
            Command::OpenHelp => self.open_help().await,
            Command::OpenPalette { tab } => self.open_palette(tab).await,
            Command::CommandPrompt(spec) => self.open_prompt(spec),
            Command::Confirm { prompt, then } => self.ask(prompt, *then),
            Command::DisplayPanes { duration_ms } => self.open_panes(duration_ms),
            Command::ClockMode { target } => self.open_clock(target),
            Command::Popup(PopupCommand::Display(opts)) => self.open_popup(opts),
            Command::Popup(PopupCommand::Menu { title, x, y, items }) => {
                self.open_menu(title, x, y, items)
            }
            Command::ShowMessages => {
                let lines = self.chrome.message_log.clone();
                self.open_overlay(Overlay::text("messages", lines)).await
            }
            Command::ChooseBuffer => self.open_overlay(Overlay::buffers()).await,
            other => return Err(other),
        };
        Ok(flow)
    }

    /// `select-window -t name`: selects the window with that name, or reports it missing.
    async fn select_window_named(&mut self, name: String) -> Flow {
        let found = self
            .layout
            .tabs
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.index);
        match found {
            Some(index) => self.simple(Simple::SelectWindow(index)).await,
            None => self.say(format!("can't find window: {name}")),
        }
    }

    /// `rename-session`: renames the session, keeping the local name only on success.
    async fn rename_session_cmd(&mut self, name: String) -> Flow {
        let result = self.client.rename_session(&name).await;
        if result.is_ok() {
            self.session = name;
        }
        self.settle(result)
    }

    async fn buffer_command(&mut self, cmd: BufferCommand) -> Flow {
        match cmd {
            BufferCommand::ListBuffers { format } => self.list_buffers(format).await,
            BufferCommand::SetBuffer {
                name,
                append,
                rename,
                data,
            } => self.set_buffer(name, append, rename, data).await,
            BufferCommand::ShowBuffer { name } => self.show_buffer(name).await,
            BufferCommand::DeleteBuffer { name } => {
                let result = self.client.delete_buffer(name.as_deref()).await;
                self.settle(result)
            }
            BufferCommand::LoadBuffer { name, path } => self.load_buffer(name, &path).await,
            BufferCommand::SaveBuffer { name, append, path } => {
                self.save_buffer(name, append, &path).await
            }
            BufferCommand::PasteBuffer { opts } => self.paste_buffer(opts).await,
            BufferCommand::CapturePane {
                target,
                print,
                buffer,
                spec,
            } => self.capture_pane(target, print, buffer, spec).await,
        }
    }

    async fn environment_command(&mut self, cmd: EnvCommand) -> Flow {
        match cmd {
            EnvCommand::SetEnvironment { name, change } => {
                let result = self.client.set_environment(&name, change).await;
                self.settle(result)
            }
            EnvCommand::ShowEnvironment { name } => self.show_environment(name).await,
        }
    }

    async fn client_command(&mut self, cmd: ClientCommand) -> Flow {
        match cmd {
            ClientCommand::Suspend => self.suspend_client(),
            ClientCommand::ListClients { target, format } => {
                self.list_clients(target, format).await
            }
            ClientCommand::DetachClients { target, all_others } => {
                self.detach_clients(target, all_others).await
            }
            ClientCommand::RefreshClient { target, size } => {
                self.refresh_client(target, size).await
            }
            ClientCommand::ChooseClient => self.open_overlay(Overlay::clients()).await,
        }
    }

    /// Re-reads the config file. A file that fails to parse, or a keymap that fails to resolve,
    /// leaves everything unchanged. The daemon reloads the settings it owns (resume rules,
    /// restore, history limit and spawn defaults) from the same file.
    async fn reload_config(&mut self) -> Flow {
        let path = self.launch.config_path.clone().or_else(Config::path);
        let config = match Config::load_at(path.as_deref()) {
            Ok(config) => config,
            Err(e) => return self.say(format!("reload-config: {e}")),
        };
        if let Err(e) = keymap::resolve(&config) {
            return self.say(format!("reload-config: {e}"));
        }
        let daemon = self.client.reload_config(path.as_deref()).await;
        self.config = config;
        self.apply_bindings().await;
        self.layout.dirty = true;
        match daemon {
            Ok(()) => self.say("config reloaded"),
            Err(e) => self.say(format!("config reloaded in the UI only: {e}")),
        }
    }

    /// `set-option`: changes this client's option. A value that does not parse, or a prefix the
    /// keymap cannot resolve, leaves the config unchanged.
    async fn set_option(
        &mut self,
        name: &str,
        value: Option<&str>,
        unset: bool,
        quiet: bool,
    ) -> Flow {
        if quiet && !crate::options::is_known(name) {
            return Flow::Continue;
        }
        let mut config = self.config.clone();
        let result = match value {
            Some(value) if !unset => crate::options::set(&mut config, name, value),
            _ => crate::options::unset(&mut config, name),
        };
        if let Err(e) = result.and_then(|()| keymap::resolve(&config).map(|_| ())) {
            return self.say(format!("set-option: {e}"));
        }
        self.config = config;
        self.apply_bindings().await;
        Flow::Continue
    }

    /// `show-options`: every option in an overlay, or one on the status row.
    async fn show_options(&mut self, name: Option<&str>, value_only: bool, quiet: bool) -> Flow {
        let synchronize = self
            .layout
            .tabs
            .iter()
            .any(|t| t.is_active && t.synchronized);
        let synchronize = if synchronize { "on" } else { "off" };
        if name == Some("synchronize-panes") {
            return if value_only {
                self.say(synchronize)
            } else {
                self.say(format!("synchronize-panes {synchronize}"))
            };
        }
        let Some(name) = name else {
            let mut options = crate::options::list(&self.config);
            options.push(("synchronize-panes", synchronize.to_string()));
            options.sort_by_key(|(name, _)| *name);
            let lines = options
                .into_iter()
                .map(|(name, value)| format!("{name} {value}"))
                .collect();
            return self.open_overlay(Overlay::text("options", lines)).await;
        };
        match crate::options::get(&self.config, name) {
            Ok(value) if value_only => self.say(value),
            Ok(value) => self.say(format!("{name} {value}")),
            Err(_) if quiet && !crate::options::is_known(name) => Flow::Continue,
            Err(e) => self.say(format!("show-options: {e}")),
        }
    }

    /// Rebuilds the keymap: the config's bindings, then the session's `bind-key` / `unbind-key`
    /// changes. A binding that does not apply is reported and skipped.
    pub async fn apply_bindings(&mut self) {
        let mut problems = Vec::new();
        let mut keymap = match keymap::resolve(&self.config) {
            Ok(keymap) => keymap,
            Err(e) => {
                problems.push(e);
                self.keymap.clone()
            }
        };
        for (name, line) in self.client.get_bindings().await.unwrap_or_default() {
            let (table, key) = name.split_once(':').unwrap_or(("prefix", &name));
            let table = keymap::Table::from_name(table).unwrap_or(keymap::Table::Prefix);
            if let Err(e) = keymap.bind(table, key, &line) {
                problems.push(e);
            }
        }
        self.keymap = keymap;
        self.layout.dirty = true;
        if let Some(first) = problems.first() {
            self.say(first.clone());
        }
    }

    /// `bind-key` / `unbind-key`: changes this keymap and records the binding in the session, so
    /// terminals attaching later have it too.
    async fn bind_key(&mut self, table: keymap::Table, key: &str, line: &str) -> Flow {
        if let Err(e) = self.keymap.bind(table, key, line) {
            return self.say(e);
        }
        let name = format!("{}:{key}", table.name());
        let result = self.client.set_binding(&name, Some(line)).await;
        self.settle(result)
    }

    /// Moves the active window to `to` (the window numbering `base-index` sets).
    async fn move_window(&mut self, to: usize) -> Flow {
        let Some(to) = to.checked_sub(self.config.base_index) else {
            return self.say(format!("can't find window: {to}"));
        };
        let Some(from) = self
            .layout
            .tabs
            .iter()
            .find(|t| t.is_active)
            .map(|t| t.index)
        else {
            return self.say("no active window");
        };
        let result = self.client.move_window(from, to).await;
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
        let name = target.unwrap_or_else(|| self.active_pane.clone());
        let command = (!command.is_empty()).then_some(command);
        let result = self.client.respawn(&name, kill, cwd, command).await;
        self.settle(result)
    }

    /// `respawn-window`: restarts every pane of the window (the active window unless numbered).
    async fn respawn_window(
        &mut self,
        target: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    ) -> Flow {
        let index = match target {
            None => None,
            Some(number) => match number.checked_sub(self.config.base_index) {
                Some(index) => Some(index),
                None => return self.say(format!("can't find window: {number}")),
            },
        };
        let command = (!command.is_empty()).then_some(command);
        let result = self.client.respawn_window(index, kill, cwd, command).await;
        self.settle(result)
    }

    /// `find-window`: the sessions and windows palette with `pattern` typed into its filter.
    async fn find_window(&mut self, pattern: &str) -> Flow {
        let view = super::overlay::PaletteView::new(
            crate::tui::command::PaletteTab::Sessions,
            self.keymap.help_entries(),
            &self.session,
        )
        .with_query(pattern);
        self.open_overlay(Overlay::palette(view)).await
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
        let dst = dst.unwrap_or_else(|| self.active_pane.clone());
        let result = self
            .client
            .join_pane(&src, &dst, side_by_side, detach)
            .await;
        self.settle(result)
    }

    /// `swap-window`: exchanges window `src` (the active one unless given) with `dst`, both in the
    /// numbering `base-index` sets.
    async fn swap_window(&mut self, src: Option<usize>, dst: usize) -> Flow {
        let base = self.config.base_index;
        let slot = |number: usize| number.checked_sub(base);
        let from = match src {
            Some(number) => slot(number),
            None => self
                .layout
                .tabs
                .iter()
                .find(|t| t.is_active)
                .map(|t| t.index),
        };
        let (Some(from), Some(to)) = (from, slot(dst)) else {
            return self.say("can't find window");
        };
        let result = self.client.swap_window(from, to).await;
        self.settle(result)
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
            Some(index) => self.simple(Simple::SelectWindow(index)).await,
            None => self.say(format!(
                "no {} window with an alert",
                if forward { "next" } else { "previous" }
            )),
        }
    }

    /// Commands one after the other, until one quits. A command that waits on the daemon starts a
    /// job, and the commands after it run when the job finishes; a background one lets them run at
    /// once. `if-shell -F` decides its branch here, without waiting.
    async fn run_sequence(&mut self, commands: Vec<Command>) -> Flow {
        let mut queue = VecDeque::from(commands);
        while let Some(cmd) = queue.pop_front() {
            match self.step(cmd).await {
                Step::Next => {}
                Step::Branch(cmd) => queue.push_front(cmd),
                Step::Quit => return Flow::Quit,
                Step::Background(work) => self.spawn_job(work, Vec::new()),
                Step::Wait(work) => {
                    self.spawn_job(work, queue.drain(..).collect());
                    return Flow::Continue;
                }
            }
        }
        Flow::Continue
    }

    /// Plans one command of a sequence: the job it starts, the branch it runs, or it runs now.
    async fn step(&mut self, cmd: Command) -> Step {
        match cmd {
            Command::Script(ScriptCommand::IfShell {
                format: true,
                target,
                condition,
                then,
                otherwise,
                ..
            }) => self.format_branch(target.as_deref(), &condition, *then, otherwise.map(|o| *o)),
            Command::Script(ScriptCommand::RunShell {
                background,
                target,
                delay_ms,
                command,
            }) => {
                let command = frame::expand_for(self, target.as_deref(), &command);
                let work = Work::Shell {
                    command,
                    target,
                    background,
                    delay: delay_ms.map(Duration::from_millis),
                };
                Self::scheduled(background, work)
            }
            Command::Script(ScriptCommand::IfShell {
                background,
                target,
                condition,
                then,
                otherwise,
                ..
            }) => {
                let condition = frame::expand_for(self, target.as_deref(), &condition);
                let work = Work::If {
                    condition,
                    target,
                    then,
                    otherwise,
                };
                Self::scheduled(background, work)
            }
            Command::Script(ScriptCommand::WaitFor { op, channel }) => {
                Step::Wait(Work::Wait { op, channel })
            }
            cmd => self.run_now(cmd).await,
        }
    }

    /// `if-shell -F`: the truth of the format picks the branch to run next.
    fn format_branch(
        &self,
        target: Option<&str>,
        condition: &str,
        then: Command,
        otherwise: Option<Command>,
    ) -> Step {
        let value = frame::expand_for(self, target, condition);
        let branch = if crate::format::truthy(&value) {
            Some(then)
        } else {
            otherwise
        };
        branch.map_or(Step::Next, Step::Branch)
    }

    /// Runs `cmd` now: a quit ends the sequence, anything else lets it go on.
    async fn run_now(&mut self, cmd: Command) -> Step {
        if Box::pin(self.run(cmd)).await == Flow::Quit {
            Step::Quit
        } else {
            Step::Next
        }
    }

    /// `work` as a background job, or one the commands after it wait for.
    fn scheduled(background: bool, work: Work) -> Step {
        if background {
            Step::Background(work)
        } else {
            Step::Wait(work)
        }
    }

    /// The script commands: the ones that wait on the daemon run as a sequence of one, the rest act
    /// at once.
    async fn run_script(&mut self, script: ScriptCommand) -> Flow {
        match script {
            ScriptCommand::RunShell { .. }
            | ScriptCommand::IfShell { .. }
            | ScriptCommand::WaitFor { .. } => {
                self.run_sequence(vec![Command::Script(script)]).await
            }
            ScriptCommand::SetHook {
                name,
                command,
                append,
            } => {
                let req = Request::Script(ScriptRequest::SetHook {
                    name,
                    command,
                    append,
                });
                let result = self.client.call_ok("SetHook", req).await;
                self.settle(result)
            }
            ScriptCommand::RunHook { name } => {
                let req = Request::Script(ScriptRequest::RunHook { name });
                let result = self.client.call_ok("RunHook", req).await;
                self.settle(result)
            }
            ScriptCommand::ShowHooks => self.show_hooks().await,
            ScriptCommand::SetAlias { name, command } => self.set_alias(name, command).await,
            ScriptCommand::SourceFile { paths, quiet } => self.source_file(paths, quiet).await,
        }
    }

    /// Starts a job for a command that waits on the daemon; `rest` are the commands that follow it.
    fn spawn_job(&self, work: Work, rest: Vec<Command>) {
        let endpoint = self.endpoint.clone();
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            let finished = job::run(&endpoint, work, rest).await;
            let _ = jobs.send(finished);
        });
    }

    /// Applies a finished job: its output goes to the status row, and the commands that followed
    /// its command run.
    pub async fn finish_job(&mut self, finished: Job) -> Flow {
        match finished.outcome {
            Ok(lines) => {
                for line in lines {
                    self.say(line);
                }
            }
            Err(e) => {
                self.say(e);
            }
        }
        self.run_sequence(finished.then).await
    }

    async fn show_hooks(&mut self) -> Flow {
        let lines = match self
            .client
            .call(Request::Script(ScriptRequest::ShowHooks))
            .await
        {
            Ok(Response::Hooks(hooks)) => Ok(hooks
                .into_iter()
                .map(|(name, command)| format!("{name} {command}"))
                .collect()),
            Ok(Response::Error(e)) | Err(e) => Err(e),
            Ok(other) => Err(format!("unexpected response to show-hooks: {other:?}")),
        };
        self.show_lines("hooks", lines).await
    }

    /// Opens the overlay `title` with `lines`, or says the error.
    async fn show_lines(&mut self, title: &str, lines: Result<Vec<String>, String>) -> Flow {
        match lines {
            Ok(lines) => self.open_overlay(Overlay::text(title, lines)).await,
            Err(e) => self.say(e),
        }
    }

    /// `set -s command-alias`: the alias is set in the daemon and here, and bindings made from now
    /// on expand it.
    async fn set_alias(&mut self, name: String, command: Option<String>) -> Flow {
        let req = Request::Script(ScriptRequest::SetAlias {
            name: name.clone(),
            command: command.clone(),
        });
        let result = self.client.call_ok("SetAlias", req).await;
        if result.is_ok() {
            match &command {
                Some(command) => self.config.alias.insert(name, command.clone()),
                None => self.config.alias.remove(&name),
            };
            self.apply_bindings().await;
        }
        self.settle(result)
    }

    /// `source-file`: runs the commands of each file, in order. A line that does not parse is
    /// reported and skipped; a missing file is reported unless `-q` silences it.
    async fn source_file(&mut self, paths: Vec<String>, quiet: bool) -> Flow {
        let vars = crate::format::session_vars(
            &self.session,
            self.attachment.times,
            self.attachment.attached,
        );
        for path in paths {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if quiet && e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    self.say(format!("{path}: {e}"));
                    continue;
                }
            };
            let lines = match crate::source::lines(&text, |fmt| crate::format::expand(fmt, &vars)) {
                Ok(lines) => lines,
                Err(e) => {
                    self.say(format!("{path}: {e}"));
                    continue;
                }
            };
            for (line, text) in lines {
                match parse_in(&text, &self.config.alias) {
                    Ok(command) => {
                        if self.run_sequence(vec![command]).await == Flow::Quit {
                            return Flow::Quit;
                        }
                    }
                    Err(e) => {
                        self.say(format!("{path}:{line}: {e}"));
                    }
                }
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

    /// `list-buffers`: one line per paste buffer, newest first, each rendered by `format`.
    async fn list_buffers(&mut self, format: Option<String>) -> Flow {
        let lines: Result<Vec<String>, String> =
            self.client
                .list_buffers()
                .await
                .map(|buffers| match format {
                    Some(format) => buffers
                        .iter()
                        .map(|buffer| expand(&format, &buffer_vars(buffer)))
                        .collect(),
                    None => buffers.iter().map(buffer_summary).collect(),
                });
        self.show_lines("list-buffers", lines).await
    }

    /// `show-buffer`: the text of a paste buffer, in an overlay.
    async fn show_buffer(&mut self, name: Option<String>) -> Flow {
        let lines: Result<Vec<String>, String> = self
            .client
            .get_buffer(name.as_deref())
            .await
            .map(|buffer| buffer.data.lines().map(str::to_string).collect());
        self.show_lines("show-buffer", lines).await
    }

    /// `set-buffer`: stores `data`, appending with `append`, or renames the buffer to `rename`.
    async fn set_buffer(
        &mut self,
        name: Option<String>,
        append: bool,
        rename: Option<String>,
        data: Option<String>,
    ) -> Flow {
        let result = match (rename, data) {
            (Some(to), None) => self.client.rename_buffer(name.as_deref(), &to).await,
            (Some(_), Some(_)) => Err("set-buffer: -n takes no data".to_string()),
            (None, Some(data)) => self.client.set_buffer(name.as_deref(), &data, append).await,
            (None, None) => Err("set-buffer: no data given".to_string()),
        };
        self.settle(result)
    }

    /// `load-buffer`: a file's contents become a paste buffer.
    async fn load_buffer(&mut self, name: Option<String>, path: &str) -> Flow {
        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(e) => return self.say(format!("load-buffer: {path}: {e}")),
        };
        let result = self.client.set_buffer(name.as_deref(), &data, false).await;
        self.settle(result)
    }

    /// `save-buffer`: a paste buffer is written to a file.
    async fn save_buffer(&mut self, name: Option<String>, append: bool, path: &str) -> Flow {
        if path == "-" {
            return self.say("save-buffer: - (stdout) works only from the command line");
        }
        let buffer = match self.client.get_buffer(name.as_deref()).await {
            Ok(buffer) => buffer,
            Err(e) => return self.say(format!("save-buffer: {e}")),
        };
        let written = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .and_then(|mut file| file.write_all(buffer.data.as_bytes()));
        match written {
            Ok(()) => self.say(format!("saved {} to {path}", buffer.name)),
            Err(e) => self.say(format!("save-buffer: {path}: {e}")),
        }
    }

    /// `paste-buffer`: the buffer goes into the pane, which must be in this session.
    async fn paste_buffer(&mut self, opts: PasteBufferOpts) -> Flow {
        let target = (!opts.pane.is_empty()).then(|| opts.pane.clone());
        let (temp, pane) = match self.resolve_target(target.as_deref()).await {
            Ok(found) => found,
            Err(e) => return self.say(e),
        };
        if temp.is_some() {
            return self.say("paste-buffer: the pane must be in this session");
        }
        let result = self
            .client
            .paste_buffer(PasteBufferOpts { pane, ..opts })
            .await;
        self.settle(result)
    }

    /// `capture-pane`: the pane's text, in an overlay with `-p`, else stored in a paste buffer.
    async fn capture_pane(
        &mut self,
        target: Option<String>,
        print: bool,
        buffer: Option<String>,
        spec: CaptureSpec,
    ) -> Flow {
        let (mut temp, pane) = match self.resolve_target(target.as_deref()).await {
            Ok(found) => found,
            Err(e) => return self.say(e),
        };
        let text = match pick(&mut temp, &mut self.client)
            .capture_pane(&pane, spec)
            .await
        {
            Ok(text) => text,
            Err(e) => return self.say(e),
        };
        if print {
            let lines = text.lines().map(str::to_string).collect();
            return self
                .open_overlay(Overlay::text("capture-pane", lines))
                .await;
        }
        let result = self
            .client
            .set_buffer(buffer.as_deref(), &text, false)
            .await;
        self.settle(result)
    }

    /// `show-environment`: the session environment, or one variable, in an overlay.
    async fn show_environment(&mut self, name: Option<String>) -> Flow {
        let lines: Result<Vec<String>, String> = self
            .client
            .show_environment(name.as_deref())
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| match entry.value {
                        Some(value) => format!("{}={value}", entry.name),
                        None => format!("-{}", entry.name),
                    })
                    .collect()
            });
        self.show_lines("show-environment", lines).await
    }

    /// `display-popup`: the command starts on a pty in a box over the panes. Its directory is `-d`,
    /// else the directory of the `-t` pane, else the active pane's.
    fn open_popup(&mut self, mut opts: PopupOpts) -> Flow {
        if opts.cwd.is_none() {
            let target = opts
                .target
                .clone()
                .unwrap_or_else(|| self.active_pane.clone());
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
            .unwrap_or_else(|| "/bin/sh".to_string());
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
            .map(|raw| super::frame::expand_active(self, &raw))
            .unwrap_or_default();
        let screen = (self.viewport.0, self.rows.screen_h);
        self.mode = Mode::Menu(Box::new(MenuState::new(title, items, x, y, screen)));
        Flow::Continue
    }

    /// `suspend-client`: the event loop stops the process once the command has run.
    fn suspend_client(&mut self) -> Flow {
        if cfg!(unix) {
            self.chrome.suspend_requested = true;
            Flow::Continue
        } else {
            self.say("suspend-client is not supported on this platform")
        }
    }

    async fn open_palette(&mut self, tab: Option<crate::tui::command::PaletteTab>) -> Flow {
        let entries = self.keymap.help_entries();
        let view =
            super::overlay::PaletteView::new(tab.unwrap_or_default(), entries, &self.session);
        self.open_overlay(Overlay::palette(view)).await
    }

    /// `command-prompt`: one question per prompt, each starting with its initial text. The answers
    /// fill the template and the result runs; without a template the typed line runs.
    fn open_prompt(&mut self, spec: PromptSpec) -> Flow {
        let questions = self.prompt_questions(&spec);
        self.mode = Mode::Prompt(PromptRun::new(questions, spec.template, spec.single_key));
        Flow::Continue
    }

    /// The questions a `command-prompt` asks, with formats expanded when `-F` is given.
    fn prompt_questions(&self, spec: &PromptSpec) -> VecDeque<Question> {
        let fmt = |text: &str| {
            if spec.expand {
                super::frame::expand_active(self, text)
            } else {
                text.to_string()
            }
        };
        let default_label = super::prompt::default_label(spec.template.as_deref());
        (0..spec.prompts.len().max(1))
            .map(|i| {
                let label = spec.prompts.get(i).or(spec.prompts.last());
                Question {
                    label: fmt(label.map_or(default_label.as_str(), String::as_str)),
                    initial: fmt(spec.initial.get(i).map_or("", String::as_str)),
                    kind: spec.kind,
                    numeric: spec.numeric,
                }
            })
            .collect()
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
            Simple::NextWindow => self.client.next_window().await,
            Simple::PrevWindow => self.client.prev_window().await,
            Simple::LastWindow => self.client.last_window().await,
            Simple::SelectWindow(index) => self.client.select_window(index).await,
            Simple::KillWindow(index) => self.client.kill_window(index).await,
            Simple::RenameWindow(name) => self.client.rename_window(None, &name).await,
            Simple::FocusNext => self.client.next_pane().await,
            Simple::FocusLast(keep_zoom) => self.client.focus_last_with(keep_zoom).await,
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
        let infos = ipc::session_infos(&self.launch.socket_dir).await;
        let live: Vec<_> = infos
            .iter()
            .filter(|i| i.panes > 0 || i.endpoint == self.endpoint)
            .collect();
        let endpoints: Vec<String> = live.iter().map(|i| i.endpoint.clone()).collect();
        let target = neighbor_session(&endpoints, &self.endpoint, next)
            .and_then(|endpoint| live.into_iter().find(|i| i.endpoint == endpoint));
        match target {
            Some(info) => {
                let result = self.attach_endpoint(&info.endpoint, &info.name).await;
                self.settle(result)
            }
            None => self.say("no other session"),
        }
    }

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

    async fn select_dir(&mut self, side: Side, keep_zoom: bool) -> Flow {
        match find_directional(&self.panes, &self.active_pane, side) {
            Some(pane) => {
                let result = self.client.focus_with(&pane, keep_zoom).await;
                self.settle(result)
            }
            None => Flow::Continue,
        }
    }

    async fn split_window(
        &mut self,
        flags: SplitFlags,
        argv: Vec<String>,
        cwd: Option<String>,
    ) -> Flow {
        let name = self.fresh_name("pane");
        let spawn = self.spawn_opts(name, argv, cwd).await;
        let opts = SplitOpts {
            target: self.active_target(),
            spawn,
            direction: if flags.side_by_side {
                Direction::Horizontal
            } else {
                Direction::Vertical
            },
            percent: None,
            focus: !flags.detach,
            keep_zoom: flags.keep_zoom,
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
            match &self.yank.last_copy {
                Some(s) => s.clone(),
                None => match &self.yank.clipboard {
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

    async fn mark_pane(&mut self, target: Option<&str>) -> Flow {
        let result = self.client.mark_pane(target, false).await;
        self.settle(result)
    }

    async fn clear_mark(&mut self) -> Flow {
        let result = self.client.mark_pane(None, true).await;
        self.settle(result)
    }

    /// `select-pane -d` / `-e`: the pane's input is off or back on.
    async fn input_off(&mut self, target: Option<String>, off: bool) -> Flow {
        let result = self.client.input_off(target.as_deref(), off).await;
        self.settle(result)
    }

    /// `resize-pane -Z`: the daemon zooms or unzooms the active window.
    async fn toggle_zoom(&mut self) -> Flow {
        let result = self.client.zoom(None, None).await;
        self.settle(result)
    }

    /// `set-option [-g] synchronize-panes`: the active window, or every window with `-g`.
    async fn set_synchronize(&mut self, all: bool, on: bool) -> Flow {
        let windows: Vec<usize> = self
            .layout
            .tabs
            .iter()
            .filter(|t| all || t.is_active)
            .map(|t| t.index)
            .collect();
        let mut result = Ok(());
        for window in windows {
            result = self.client.synchronize(Some(window), Some(on)).await;
            if result.is_err() {
                break;
            }
        }
        self.settle(result)
    }

    /// `copy-mode [-equ] [-t pane]`. A pane that is not focused gets its copy state parked, so
    /// focus does not move. A pane already in copy mode stays so (`-u` still pages up).
    async fn copy_mode_cmd(&mut self, o: CopyModeOpts) -> Flow {
        let raw = o.target.clone().unwrap_or_else(|| self.active_pane.clone());
        let (_, name) = split_target(&raw);
        let name = name.to_string();
        if name.is_empty() {
            return self.say("no active pane");
        }
        if !self.panes.iter().any(|(pane, ..)| *pane == name) {
            return self.say(format!("no pane {raw}"));
        }
        if o.quit {
            if self.in_copy_mode(&name) {
                if name == self.active_pane {
                    self.mode = Mode::Normal;
                }
                self.parked_copy.remove(&name);
                self.release_scrollback(&name);
            }
            return Flow::Continue;
        }
        let page_up = CopyAction::Move {
            motion: Motion::PageUp,
            cancel_at_bottom: false,
        };
        if self.in_copy_mode(&name) {
            return if o.page_up {
                self.run_copy(&name, &page_up, 1).await
            } else {
                Flow::Continue
            };
        }
        self.load_scrollback(&name).await;
        let mut cs = CopyState::enter(self.vt.get(&name).map(|vt| vt.grid()));
        cs.wheel_exit = o.exit_at_bottom;
        if name == self.active_pane {
            self.mode = Mode::Copy(cs);
        } else {
            self.parked_copy.insert(name.clone(), cs);
        }
        if o.page_up {
            return self.run_copy(&name, &page_up, 1).await;
        }
        Flow::Continue
    }

    /// Whether pane `name` is in copy mode in this terminal: focused in it, or parked.
    pub fn in_copy_mode(&self, name: &str) -> bool {
        (name == self.active_pane && matches!(self.mode, Mode::Copy(_)))
            || self.parked_copy.contains_key(name)
    }

    /// `send-keys -X`: a copy-mode command in the copy mode of `target`, the active pane when
    /// `None`. Nothing happens in a terminal that does not have that pane in copy mode.
    async fn copy_command(
        &mut self,
        target: Option<String>,
        action: CopyAction,
        count: usize,
    ) -> Flow {
        let raw = target.unwrap_or_else(|| self.active_pane.clone());
        let (_, name) = split_target(&raw);
        let name = name.to_string();
        self.run_copy(&name, &action, count).await
    }

    /// Runs `action` `count` times in the copy mode of pane `name`, stopping at the first outcome
    /// that is not a plain move.
    async fn run_copy(&mut self, name: &str, action: &CopyAction, count: usize) -> Flow {
        let active = name == self.active_pane;
        let mut cs = if active {
            match std::mem::replace(&mut self.mode, Mode::Normal) {
                Mode::Copy(cs) => cs,
                other => {
                    self.mode = other;
                    return Flow::Continue;
                }
            }
        } else {
            match self.parked_copy.remove(name) {
                Some(cs) => cs,
                None => return Flow::Continue,
            }
        };
        cs.wrap_search = self.config.wrap_search;
        let size = self.copy_pane_size(name);
        let grid = self.vt.get(name).map(|vt| vt.grid());
        let mut outcome = CopyOutcome::Stay;
        for _ in 0..count.max(1) {
            outcome = apply_action(&mut cs, action, grid, size);
            if outcome != CopyOutcome::Stay {
                break;
            }
        }
        self.apply_copy_outcome(name, active, cs, outcome);
        Flow::Continue
    }

    /// The size of pane `name` in cells, which copy-mode motions are clamped to.
    fn copy_pane_size(&self, name: &str) -> (u16, u16) {
        match self.active_rect {
            Some(rect) if name == self.active_pane => (rect.width, rect.height),
            _ => self.pane_dims.get(name).copied().unwrap_or((80, 24)),
        }
    }

    /// Carries out what a copy-mode command decided for pane `name` and puts its copy state back:
    /// `active` says whether the pane is the focused one, else its state is parked.
    pub fn apply_copy_outcome(
        &mut self,
        name: &str,
        active: bool,
        cs: CopyState,
        outcome: CopyOutcome,
    ) {
        let (text, pipe, exit) = match outcome {
            CopyOutcome::Stay => (None, None, false),
            CopyOutcome::Exit => (None, None, true),
            CopyOutcome::Yank(text) => (Some(text), None, true),
            CopyOutcome::Copy(text) => (Some(text), None, false),
            CopyOutcome::Pipe {
                text,
                command,
                exit,
            } => (Some(text), Some(command), exit),
            CopyOutcome::SearchFailed(query) => {
                self.say(format!("Pattern not found: {query}"));
                (None, None, false)
            }
        };
        if let Some(text) = &text {
            if self.config.set_clipboard {
                self.yank.clipboard.copy(text);
            }
            self.yank.yanks.push(text.clone());
            self.yank.last_copy = Some(text.clone());
        }
        if let (Some(command), Some(text)) = (pipe, &text) {
            pipe_text(&command, text);
        }
        if exit {
            if active {
                self.mode = Mode::Normal;
            }
            self.release_scrollback(name);
        } else if active {
            self.mode = Mode::Copy(cs);
        } else {
            self.parked_copy.insert(name.to_string(), cs);
        }
    }

    async fn kill_server(&mut self) -> Flow {
        let _ = self.client.shutdown().await;
        Flow::Quit
    }

    async fn send_prefix(&mut self) -> Flow {
        let bytes =
            self.encode_for_active(|modes, policy| self.keymap.prefix().encode(modes, policy));
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
            socket_dir: &self.launch.socket_dir,
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

    /// Spawn options for a new pane: the given argv in the given or active cwd. With no argv the
    /// daemon starts the session's default command (`default-command`, else `default-shell`).
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
