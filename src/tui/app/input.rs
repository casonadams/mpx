use crossterm::event::{Event, KeyEvent, KeyEventKind, MouseEventKind};
use std::time::{Duration, Instant};

use super::pacing::TYPING_FRAMES;
use super::state::{App, Flow, Mode};

const REPEAT_WINDOW: Duration = Duration::from_millis(500);
const HISTORY_CAP: usize = 100;
/// Status-row messages kept for `show-messages`.
const MESSAGE_LOG_CAP: usize = 100;

/// The last of a run of mouse drags already queued after `first`, and the first queued event that
/// is not part of the run, to handle next. A fast drag sends many motions; only the latest
/// position matters.
pub fn latest_drag(
    first: Event,
    mut queued: impl FnMut() -> Option<Event>,
) -> (Event, Option<Event>) {
    let is_drag =
        |ev: &Event| matches!(ev, Event::Mouse(m) if matches!(m.kind, MouseEventKind::Drag(_)));
    if !is_drag(&first) {
        return (first, None);
    }
    let mut last = first;
    while let Some(next) = queued() {
        if !is_drag(&next) {
            return (last, Some(next));
        }
        last = next;
    }
    (last, None)
}

impl App {
    pub async fn on_event(&mut self, ev: Event) -> Flow {
        if matches!(ev, Event::Key(_) | Event::Mouse(_) | Event::Paste(_)) {
            self.touch_client().await;
        }
        let flow = self.handle_event(ev).await;
        self.after_input().await;
        flow
    }

    /// Work that follows every input or stream batch: copies made reach the paste buffers.
    pub(super) async fn after_input(&mut self) {
        self.ring_bells();
        for text in std::mem::take(&mut self.yank.yanks) {
            if let Err(e) = self.client.set_buffer(None, &text, false).await {
                self.say(format!("set-buffer: {e}"));
            }
        }
    }

    async fn handle_event(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.pacing.last_key = Some(Instant::now());
                self.pacing.key_frames = TYPING_FRAMES;
                self.on_key(key).await
            }
            Event::Mouse(mouse) => self.on_mouse(mouse).await,
            Event::Paste(text) => self.on_paste(&text).await,
            Event::FocusGained => self.on_focus(true).await,
            Event::FocusLost => self.on_focus(false).await,
            Event::Resize(..) => {
                self.layout.dirty = true;
                self.canvas.invalidate();
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    /// Records a message for the status row; it expires after a few seconds.
    pub fn say(&mut self, text: impl Into<String>) -> Flow {
        let text = text.into();
        self.chrome.message_log.push(text.clone());
        if self.chrome.message_log.len() > MESSAGE_LOG_CAP {
            self.chrome.message_log.remove(0);
        }
        self.interaction.message = Some((text, Instant::now()));
        Flow::Continue
    }

    /// Shows a failed daemon request on the status row, as `<what>: <error>`.
    pub fn report<T, E: std::fmt::Display>(&mut self, what: &str, result: Result<T, E>) {
        if let Err(e) = result {
            self.say(format!("{what}: {e}"));
        }
    }

    /// The live message, if one was set within the last few seconds.
    pub fn live_message(&self) -> Option<&str> {
        self.interaction
            .message
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_millis(self.config.display_time_ms))
            .map(|(text, _)| text.as_str())
    }

    pub fn remember(&mut self, line: &str) {
        if self.interaction.history.last().map(String::as_str) != Some(line) {
            self.interaction.history.push(line.to_string());
            if self.interaction.history.len() > HISTORY_CAP {
                self.interaction.history.remove(0);
            }
        }
    }

    pub fn repeating(&self) -> bool {
        self.interaction
            .repeat_until
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn arm_repeat(&mut self) {
        self.interaction.repeat_until = Some(Instant::now() + REPEAT_WINDOW);
    }

    /// Enters prefix mode; the shortcut popup follows after the configured delay.
    pub fn enter_prefix(&mut self) {
        self.mode = Mode::Prefix;
        self.interaction.prefix_at = Some(Instant::now());
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Flow {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key).await,
            Mode::Prefix => self.prefix_key(key).await,
            Mode::Copy(cs) => self.copy_key(cs, key).await,
            Mode::Overlay(ov) => self.overlay_key(ov, key).await,
            Mode::Prompt(p) => self.prompt_key(p, key).await,
            Mode::Confirm(prompt, cmd) => self.confirm_key(prompt, cmd, key).await,
            Mode::Panes { .. } => self.panes_key(key).await,
            // The mode is already back to normal: any key ends clock mode and is not forwarded.
            Mode::Clock { .. } => Flow::Continue,
            Mode::Popup(popup) => self.popup_key(popup, key).await,
            Mode::Menu(menu) => self.menu_key(menu, key).await,
        }
    }
}
