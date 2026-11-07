//! Overlays, the command prompt, confirmations and the message log.
use crate::tui::app::{App, Flow, Mode};
use crate::tui::command::{Command, PaletteTab, PromptSpec};
use crate::tui::overlay::{Overlay, OverlayCtx, PaletteView};
use crate::tui::prompt::{self, PromptRun, Question};
use std::collections::VecDeque;

impl App {
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

    /// Opens the overlay `title` with `lines`, or says the error.
    pub(super) async fn show_lines(
        &mut self,
        title: &str,
        lines: Result<Vec<String>, String>,
    ) -> Flow {
        match lines {
            Ok(lines) => self.open_overlay(Overlay::text(title, lines)).await,
            Err(e) => self.say(e),
        }
    }

    pub(super) async fn open_help(&mut self) -> Flow {
        let entries = self.keymap.help_entries();
        self.open_overlay(Overlay::help(entries)).await
    }

    pub(super) async fn open_palette(&mut self, tab: Option<PaletteTab>) -> Flow {
        let entries = self.keymap.help_entries();
        let view = PaletteView::new(tab.unwrap_or_default(), entries, &self.session);
        self.open_overlay(Overlay::palette(view)).await
    }

    /// `show-messages`: the status-row messages of this session, oldest first.
    pub(super) async fn show_messages(&mut self) -> Flow {
        let lines = self.chrome.message_log.clone();
        self.open_overlay(Overlay::text("messages", lines)).await
    }

    /// `command-prompt`: one question per prompt, each starting with its initial text. The answers
    /// fill the template and the result runs; without a template the typed line runs.
    pub(super) fn open_prompt(&mut self, spec: PromptSpec) -> Flow {
        let questions = self.prompt_questions(&spec);
        self.mode = Mode::Prompt(PromptRun::new(questions, spec.template, spec.single_key));
        Flow::Continue
    }

    /// The questions a `command-prompt` asks, with formats expanded when `-F` is given.
    fn prompt_questions(&self, spec: &PromptSpec) -> VecDeque<Question> {
        let fmt = |text: &str| {
            if spec.expand {
                crate::tui::frame::expand_active(self, text)
            } else {
                text.to_string()
            }
        };
        let default_label = prompt::default_label(spec.template.as_deref());
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

    /// `confirm-before`: the question waits for a `y`, which then runs `then`.
    pub(super) fn ask(&mut self, prompt: String, then: Command) -> Flow {
        self.mode = Mode::Confirm(prompt, then);
        Flow::Continue
    }
}
