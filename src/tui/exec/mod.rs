//! Command execution: `dispatch` runs a parsed `Command` on the TUI and the daemon. `run` sends each
//! command to the handler of its family; the families are the submodules, each an `impl App`.
mod buffers;
mod config;
mod copy_mode;
mod keys;
mod layout;
mod overlays;
mod pane;
mod popups;
mod script;
mod sequence;
mod sessions;
mod spawn;
mod window;

use crate::client::Client;
use crate::tui::app::{App, Flow, Mode};
use crate::tui::command::{Command, PaneCommand, WindowCommand, read_only_allowed};
use crate::tui::frame;
use crate::tui::overlay::Overlay;

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
        if self.attachment.terminal.readonly && !read_only_allowed(&cmd) {
            return self.say("read-only client: this command would change the session");
        }
        match cmd {
            Command::Window(cmd) => self.window_command(cmd).await,
            Command::Pane(cmd) => self.pane_command(cmd).await,
            Command::Session(cmd) => self.session_command(cmd).await,
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
            Command::DisplayMessage { format } => self.display_message(format.as_deref()),
            Command::SendKeys { target, keys } => self.send_keys(target, keys).await,
            Command::Paste {
                target,
                text,
                submit,
            } => self.paste(target, text, submit).await,
            Command::SendPrefix => self.send_prefix().await,
            Command::Zoom => self.toggle_zoom().await,
            Command::SetSynchronize { all, on } => self.set_synchronize(all, on).await,
            Command::CopyMode(opts) => self.copy_mode_cmd(opts).await,
            Command::CopyCommand {
                target,
                action,
                count,
            } => self.copy_command(target, action, count).await,
            Command::Detach => Flow::Quit,
            Command::KillServer => self.kill_server().await,
            Command::ReloadConfig => self.reload_config().await,
            Command::OpenTree => self.open_overlay(Overlay::tree()).await,
            Command::OpenHelp => self.open_help().await,
            Command::OpenPalette { tab } => self.open_palette(tab).await,
            Command::CommandPrompt(spec) => self.open_prompt(spec),
            Command::Confirm { prompt, then } => self.ask(prompt, *then),
            Command::Sequence(commands) => Box::pin(self.run_sequence(commands)).await,
            Command::Bind { table, key, line } => self.bind_key(table, &key, &line).await,
            Command::DisplayPanes { duration_ms } => self.open_panes(duration_ms),
            Command::ClockMode { target } => self.open_clock(target),
            Command::ShowMessages => self.show_messages().await,
            Command::ChooseBuffer => self.open_overlay(Overlay::buffers()).await,
            Command::Buffer(cmd) => self.buffer_command(cmd).await,
            Command::Environment(cmd) => self.environment_command(cmd).await,
            Command::Client(cmd) => self.client_command(cmd).await,
            Command::Popup(cmd) => self.popup_command(cmd),
            Command::Script(cmd) => self.run_script(cmd).await,
        }
    }

    fn display_message(&mut self, format: Option<&str>) -> Flow {
        let text = frame::expand_active(self, format.unwrap_or(DEFAULT_DISPLAY_MESSAGE));
        self.say(text)
    }

    /// Reports a failed client call on the status row; success leaves no message.
    pub fn settle<T>(&mut self, result: Result<T, String>) -> Flow {
        match result {
            Ok(_) => Flow::Continue,
            Err(e) => self.say(e),
        }
    }

    /// The index of the active window in the daemon's numbering.
    fn active_window(&self) -> Option<usize> {
        self.layout
            .tabs
            .iter()
            .find(|t| t.is_active)
            .map(|t| t.index)
    }

    /// `target` when given, else the active pane.
    fn target_or_active(&self, target: Option<String>) -> String {
        target.unwrap_or_else(|| self.active_pane.clone())
    }

    /// Resolves an optional `[session:]pane` target to the pane and, for another session, a
    /// temporary connection to it. A target that cannot be reached is reported.
    async fn target_pane(
        &mut self,
        target: Option<&str>,
    ) -> Result<(Option<Client>, String), Flow> {
        self.resolve_target(target).await.map_err(|e| self.say(e))
    }
}
