//! Keyboard, paste and focus input, routed by the current mode.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, Flow, Mode};
use super::command::{self, COMMAND_NAMES, Command, PaneCommand};
use super::copy::{CopyState, apply_bound_key};
use super::keymap::Table;
use super::labels;
use super::menu::{MenuAction, MenuState};
use super::overlay::{Overlay, OverlayAction};
use super::popup::{PopupEvent, PopupState};
use super::prompt::{PromptKind, PromptRun, PromptState};
use crate::keys::{KeyPolicy, TermModes, encode_key};

/// What happens to an overlay after it handled a key.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayNext {
    /// The overlay stays open.
    Stay,
    /// The overlay closes; these commands run in order.
    Close(Vec<Command>),
    /// The paste buffer named is deleted; the overlay stays open and reloads.
    Delete(String),
}

/// Maps an overlay's key result onto the app's next step.
pub fn overlay_next(action: OverlayAction) -> OverlayNext {
    match action {
        OverlayAction::None => OverlayNext::Stay,
        OverlayAction::Close => OverlayNext::Close(Vec::new()),
        OverlayAction::Run(cmds) => OverlayNext::Close(cmds),
        OverlayAction::DeleteBuffer(name) => OverlayNext::Delete(name),
    }
}

impl App {
    pub async fn normal_key(&mut self, key: KeyEvent) -> Flow {
        if self.keymap.is_prefix(&key) {
            self.enter_prefix();
            return Flow::Continue;
        }
        if let Some(binding) = self.keymap.lookup(Table::Root, &key)
            && self.takes_root_key(&binding.command)
        {
            let cmd = binding.command.clone();
            return self.dispatch(cmd).await;
        }
        if self.repeating()
            && let Some(binding) = self.keymap.lookup(Table::Prefix, &key)
            && matches!(
                binding.command,
                Command::Pane(PaneCommand::ResizePane { .. })
            )
        {
            let cmd = binding.command.clone();
            self.arm_repeat();
            return self.dispatch(cmd).await;
        }
        self.forward_key(&key).await;
        self.pacing.forwarded = true;
        Flow::Continue
    }

    fn takes_root_key(&self, _command: &Command) -> bool {
        true
    }

    pub async fn prefix_key(&mut self, key: KeyEvent) -> Flow {
        let Some(binding) = self.keymap.lookup(Table::Prefix, &key) else {
            return Flow::Continue;
        };
        let cmd = binding.command.clone();
        if matches!(cmd, Command::Pane(PaneCommand::ResizePane { .. })) {
            self.arm_repeat();
        }
        self.dispatch(cmd).await
    }

    pub async fn copy_key(&mut self, mut cs: CopyState, key: KeyEvent) -> Flow {
        if self.keymap.is_prefix(&key) {
            self.mode = Mode::Copy(cs);
            self.park_copy();
            self.enter_prefix();
            return Flow::Continue;
        }
        let pane = self.active_rect.map_or((80, 24), |r| (r.width, r.height));
        let action = self.keymap.copy_action(self.config.mode_keys, &key);
        let grid = self.active_vt().map(|vt| vt.grid());
        cs.wrap_search = self.config.wrap_search;
        let outcome = apply_bound_key(&mut cs, &key, action.as_ref(), grid, pane);
        let active = self.active_pane.clone();
        self.apply_copy_outcome(&active, true, cs, outcome);
        Flow::Continue
    }

    pub async fn overlay_key(&mut self, mut overlay: Overlay, key: KeyEvent) -> Flow {
        match overlay_next(overlay.handle_key(key)) {
            OverlayNext::Stay => {
                overlay.follow_selection().await;
                self.mode = Mode::Overlay(overlay);
                Flow::Continue
            }
            OverlayNext::Close(cmds) => self.run_all(cmds).await,
            OverlayNext::Delete(name) => {
                self.mode = Mode::Overlay(overlay);
                let result = self.client.delete_buffer(Some(&name)).await;
                self.refresh_overlay().await;
                self.settle(result)
            }
        }
    }

    /// Runs commands in order, stopping early if one asks to quit.
    async fn run_all(&mut self, cmds: Vec<Command>) -> Flow {
        for cmd in cmds {
            if self.dispatch(cmd).await == Flow::Quit {
                return Flow::Quit;
            }
        }
        Flow::Continue
    }

    /// display-panes: a digit selects the pane numbered with it. The numbers are already gone (the
    /// mode is back to normal), so any other key just dismisses them.
    pub async fn panes_key(&mut self, key: KeyEvent) -> Flow {
        let KeyCode::Char(c) = key.code else {
            return Flow::Continue;
        };
        let Some(number) = c.to_digit(10) else {
            return Flow::Continue;
        };
        let picked = labels::pane_numbers(&self.panes, &self.nodes)
            .into_iter()
            .find(|(n, _)| *n == number as usize)
            .map(|(_, pane)| pane);
        match picked {
            Some(target) => {
                self.run(Command::Pane(PaneCommand::SelectPane {
                    target,
                    keep_zoom: false,
                }))
                .await
            }
            None => Flow::Continue,
        }
    }

    pub async fn prompt_key(&mut self, mut run: PromptRun, key: KeyEvent) -> Flow {
        match key.code {
            KeyCode::Esc => Flow::Continue,
            KeyCode::Enter => {
                let text = run.line.text();
                self.answer_prompt(run, text).await
            }
            KeyCode::Char(c)
                if run.single_key && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.answer_prompt(run, c.to_string()).await
            }
            _ => {
                self.edit_prompt(&mut run.line, &key);
                self.mode = Mode::Prompt(run);
                Flow::Continue
            }
        }
    }

    /// Records the answer to the question on screen. Once every question is answered, the command
    /// line they produce runs; until then the next question is shown.
    async fn answer_prompt(&mut self, mut run: PromptRun, text: String) -> Flow {
        match run.answer(text) {
            Some(line) => self.submit(line).await,
            None => {
                self.mode = Mode::Prompt(run);
                Flow::Continue
            }
        }
    }

    fn edit_prompt(&self, prompt: &mut PromptState, key: &KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('u'), true) => prompt.clear(),
            (KeyCode::Char('w'), true) => prompt.delete_word(),
            (KeyCode::Char(c), false) => prompt.insert(c),
            (KeyCode::Backspace, _) => prompt.backspace(),
            (KeyCode::Delete, _) => prompt.delete(),
            (KeyCode::Left, _) => prompt.left(),
            (KeyCode::Right, _) => prompt.right(),
            (KeyCode::Home, _) => prompt.home(),
            (KeyCode::End, _) => prompt.end(),
            (KeyCode::Up, _) => prompt.history_prev(&self.interaction.history),
            (KeyCode::Down, _) => prompt.history_next(&self.interaction.history),
            (KeyCode::Tab, _) => self.complete_prompt(prompt, true),
            (KeyCode::BackTab, _) => self.complete_prompt(prompt, false),
            _ => {}
        }
    }

    /// Tab completion for a prompt of its kind: command names, or window names for a window target.
    fn complete_prompt(&self, prompt: &mut PromptState, forward: bool) {
        let windows: Vec<&str> = self.layout.tabs.iter().map(|t| t.name.as_str()).collect();
        let names: &[&str] = match prompt.kind() {
            PromptKind::Command => COMMAND_NAMES,
            PromptKind::WindowTarget => &windows,
            PromptKind::Search | PromptKind::Target => &[],
        };
        if forward {
            prompt.complete(names);
        } else {
            prompt.complete_prev(names);
        }
    }

    /// Parses and runs a submitted command line; errors go to the status row.
    async fn submit(&mut self, text: String) -> Flow {
        let line = text.trim().to_string();
        if line.is_empty() {
            return Flow::Continue;
        }
        self.remember(&line);
        match command::parse_in(&line, &self.config.alias) {
            Ok(cmd) => self.dispatch(cmd).await,
            Err(e) => self.say(e),
        }
    }

    pub async fn confirm_key(&mut self, _prompt: String, cmd: Command, key: KeyEvent) -> Flow {
        if key.code == KeyCode::Char('y') {
            self.run(cmd).await
        } else {
            Flow::Continue
        }
    }

    /// Sends a key to the active pane, encoded with that pane's own input modes.
    pub async fn forward_key(&mut self, key: &KeyEvent) {
        let bytes = self.encode_for_active(|modes, policy| encode_key(key, modes, policy));
        if !bytes.is_empty() {
            let _ = self.write_typed(&bytes).await;
        }
    }

    /// Encodes with the active pane's terminal modes and the configured key policy.
    pub fn encode_for_active(
        &self,
        encode: impl FnOnce(&TermModes, KeyPolicy) -> Vec<u8>,
    ) -> Vec<u8> {
        let modes = self.modes_of(&self.active_pane);
        encode(&modes, KeyPolicy::of(&self.config))
    }

    /// Writes bytes to the active pane (focus reports, the prefix key).
    pub async fn write_active(&mut self, bytes: &[u8]) -> Result<(), String> {
        let active = [self.active_pane.clone()];
        self.send_to(&active, bytes).await
    }

    /// Writes typed bytes to the active pane, or to every running pane of its window while
    /// `synchronize-panes` is on for that window.
    async fn write_typed(&mut self, bytes: &[u8]) -> Result<(), String> {
        let targets = self.typed_targets();
        self.send_to(&targets, bytes).await
    }

    /// Sends `bytes` to each named pane. A read-only client sends nothing; an empty name is no pane.
    async fn send_to(&mut self, panes: &[String], bytes: &[u8]) -> Result<(), String> {
        if self.attachment.terminal.readonly {
            return Ok(());
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        for pane in panes.iter().filter(|p| !p.is_empty()) {
            self.client.send_input(pane, &text).await?;
        }
        Ok(())
    }

    /// The panes typed input goes to.
    fn typed_targets(&self) -> Vec<String> {
        if self.active_pane.is_empty() {
            return Vec::new();
        }
        match self.layout.tabs.iter().find(|t| t.is_active) {
            Some(tab) if tab.synchronized => self
                .nodes
                .iter()
                .filter(|n| n.window == Some(tab.index) && n.is_running())
                .map(|n| n.name.clone())
                .collect(),
            _ => vec![self.active_pane.clone()],
        }
    }

    pub async fn on_paste(&mut self, text: &str) -> Flow {
        if self.attachment.terminal.readonly {
            return Flow::Continue;
        }
        if let Mode::Prompt(prompt) = &mut self.mode {
            for c in text.chars().filter(|c| !c.is_control()) {
                prompt.line.insert(c);
            }
            return Flow::Continue;
        }
        if self.active_pane.is_empty() {
            return Flow::Continue;
        }
        let pane = self.active_pane.clone();
        let result = self.client.paste(&pane, text, false).await;
        self.settle(result)
    }

    /// The terminal's own focus changed: the active pane hears it when `focus-events` is on.
    pub async fn on_focus(&mut self, focused: bool) -> Flow {
        let active = self.active_pane.clone();
        self.send_focus(&active, focused).await;
        Flow::Continue
    }

    /// A key while a popup is open goes to its command. Once the command has ended, any key closes
    /// the popup (`on_key` has already returned the mode to normal).
    pub async fn popup_key(&mut self, mut popup: Box<PopupState>, key: KeyEvent) -> Flow {
        if !popup.exited {
            let _ = popup.send_key(&key, KeyPolicy::of(&self.config));
            self.mode = Mode::Popup(popup);
        }
        Flow::Continue
    }

    /// Output of the open popup's command, or its end: without `-E` the popup then waits for a key.
    pub fn on_popup(&mut self, event: PopupEvent) {
        let closes = match (&mut self.mode, event) {
            (Mode::Popup(popup), PopupEvent::Output(bytes)) => {
                popup.feed(&bytes);
                false
            }
            (Mode::Popup(popup), PopupEvent::Exited(_)) => {
                popup.exited = true;
                popup.close_on_exit()
            }
            _ => false,
        };
        if closes {
            self.mode = Mode::Normal;
        }
    }

    /// A key while a menu is open: its item key or the arrows pick, Esc and `q` close.
    pub async fn menu_key(&mut self, mut menu: Box<MenuState>, key: KeyEvent) -> Flow {
        match menu.handle_key(key) {
            MenuAction::Stay => {
                self.mode = Mode::Menu(menu);
                Flow::Continue
            }
            MenuAction::Close => Flow::Continue,
            MenuAction::Run(line) => match command::parse(&line) {
                Ok(cmd) => self.dispatch(cmd).await,
                Err(e) => self.say(e),
            },
        }
    }
}
