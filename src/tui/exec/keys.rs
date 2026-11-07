//! Input sent to panes: `send-keys`, `paste` and the prefix key.
use crate::tui::app::{App, Clipboard, Flow};
use crate::tui::render::read_clipboard;
use crate::tui::session::pick;

impl App {
    pub(super) async fn send_keys(&mut self, target: Option<String>, keys: Vec<String>) -> Flow {
        let (mut temp, pane) = match self.target_pane(target.as_deref()).await {
            Ok(found) => found,
            Err(flow) => return flow,
        };
        let result = pick(&mut temp, &mut self.client)
            .send_keys(&pane, keys, false)
            .await;
        self.settle(result)
    }

    /// `paste`: the text, or the newest copy when it is empty.
    pub(super) async fn paste(
        &mut self,
        target: Option<String>,
        text: String,
        submit: bool,
    ) -> Flow {
        let (mut temp, pane) = match self.target_pane(target.as_deref()).await {
            Ok(found) => found,
            Err(flow) => return flow,
        };
        let text = if text.is_empty() {
            match &self.yank.last_copy {
                Some(s) => s.clone(),
                None => match &self.yank.clipboard {
                    Clipboard::Captured(items) => items.last().cloned().unwrap_or_default(),
                    Clipboard::System => read_clipboard().unwrap_or_default(),
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

    /// `send-prefix`: the prefix key itself goes to the active pane.
    pub(super) async fn send_prefix(&mut self) -> Flow {
        let bytes =
            self.encode_for_active(|modes, policy| self.keymap.prefix().encode(modes, policy));
        let result = self.write_active(&bytes).await;
        self.settle(result)
    }
}
