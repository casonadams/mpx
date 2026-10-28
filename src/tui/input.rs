//! Keyboard, paste and focus input, routed by the current mode.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, Flow, Mode};
use super::command::{self, COMMAND_NAMES, Command};
use super::copy::{CopyOutcome, CopyState, apply_key};
use super::overlay::{Overlay, OverlayAction};
use super::prompt::PromptState;
use crate::keys::encode_key;

/// What happens to an overlay after it handled a key.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayNext {
    /// The overlay stays open.
    Stay,
    /// The overlay closes; these commands run in order.
    Close(Vec<Command>),
}

/// Maps an overlay's key result onto the app's next step.
pub fn overlay_next(action: OverlayAction) -> OverlayNext {
    match action {
        OverlayAction::None => OverlayNext::Stay,
        OverlayAction::Close => OverlayNext::Close(Vec::new()),
        OverlayAction::Run(cmds) => OverlayNext::Close(cmds),
    }
}

impl App {
    pub async fn normal_key(&mut self, key: KeyEvent) -> Flow {
        if self.keymap.is_prefix(&key) {
            self.enter_prefix();
            return Flow::Continue;
        }
        if self.repeating()
            && let Some(binding) = self.keymap.lookup(&key)
            && matches!(binding.command, Command::ResizePane { .. })
        {
            let cmd = binding.command.clone();
            self.arm_repeat();
            return self.dispatch(cmd).await;
        }
        self.forward_key(&key).await;
        Flow::Continue
    }

    pub async fn prefix_key(&mut self, key: KeyEvent) -> Flow {
        let Some(binding) = self.keymap.lookup(&key) else {
            return Flow::Continue;
        };
        let cmd = binding.command.clone();
        if matches!(cmd, Command::ResizePane { .. }) {
            self.arm_repeat();
        }
        self.dispatch(cmd).await
    }

    pub async fn copy_key(&mut self, mut cs: CopyState, key: KeyEvent) -> Flow {
        if self.keymap.is_prefix(&key) {
            self.enter_prefix();
            return Flow::Continue;
        }
        let pane = self.active_rect.map_or((80, 24), |r| (r.width, r.height));
        let outcome = apply_key(&mut cs, &key, self.active_vt().map(|vt| vt.grid()), pane);
        match outcome {
            CopyOutcome::Stay => self.mode = Mode::Copy(cs),
            CopyOutcome::Exit => {}
            CopyOutcome::Yank(text) => self.clipboard.copy(&text),
            CopyOutcome::SearchFailed(query) => {
                self.say(format!("Pattern not found: {query}"));
                self.mode = Mode::Copy(cs);
            }
        }
        Flow::Continue
    }

    pub async fn overlay_key(&mut self, mut overlay: Overlay, key: KeyEvent) -> Flow {
        match overlay_next(overlay.handle_key(key)) {
            OverlayNext::Stay => {
                self.mode = Mode::Overlay(overlay);
                Flow::Continue
            }
            OverlayNext::Close(cmds) => self.run_all(cmds).await,
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

    pub async fn prompt_key(&mut self, mut prompt: PromptState, key: KeyEvent) -> Flow {
        match key.code {
            KeyCode::Esc => Flow::Continue,
            KeyCode::Enter => self.submit(prompt.text()).await,
            _ => {
                self.edit_prompt(&mut prompt, &key);
                self.mode = Mode::Prompt(prompt);
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
            (KeyCode::Up, _) => prompt.history_prev(&self.history),
            (KeyCode::Down, _) => prompt.history_next(&self.history),
            (KeyCode::Tab, _) => prompt.complete(COMMAND_NAMES),
            (KeyCode::BackTab, _) => prompt.complete_prev(COMMAND_NAMES),
            _ => {}
        }
    }

    /// Parses and runs a submitted command line; errors go to the status row.
    async fn submit(&mut self, text: String) -> Flow {
        let line = text.trim().to_string();
        if line.is_empty() {
            return Flow::Continue;
        }
        self.remember(&line);
        match command::parse(&line) {
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
        let modes = self.modes_of(&self.active_pane.clone());
        let bytes = encode_key(key, &modes);
        if !bytes.is_empty() {
            let _ = self.write_active(&bytes).await;
        }
    }

    pub async fn write_active(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.active_pane.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        let pane = self.active_pane.clone();
        self.client.send_input(&pane, &text).await
    }

    pub async fn on_paste(&mut self, text: &str) -> Flow {
        if let Mode::Prompt(prompt) = &mut self.mode {
            for c in text.chars().filter(|c| !c.is_control()) {
                prompt.insert(c);
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

    pub async fn on_focus(&mut self, focused: bool) -> Flow {
        if self.modes_of(&self.active_pane.clone()).focus_events {
            let bytes = crate::keys::focus_event(focused);
            let _ = self.write_active(bytes).await;
        }
        Flow::Continue
    }
}
