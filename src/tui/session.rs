//! Session switching and `[session:]pane` target resolution.
use super::app::{App, Mode};
use crate::client::Client;
use crate::ipc;

/// Splits `session:pane` into its parts; a bare pane has no session.
pub fn split_target(target: &str) -> (Option<&str>, &str) {
    match target.split_once(':') {
        Some((session, pane)) if !session.is_empty() => (Some(session), pane),
        _ => (None, target),
    }
}

/// The session after (`next`) or before `current` in `names`, wrapping around.
/// `None` when there is no other session.
pub fn neighbor_session(names: &[String], current: &str, next: bool) -> Option<String> {
    if names.len() < 2 {
        return None;
    }
    let len = names.len();
    let idx = names.iter().position(|n| n == current).unwrap_or(0);
    let other = if next {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    };
    Some(names[other].clone())
}

/// Picks the temporary connection when one was opened, otherwise the attached client.
pub fn pick<'a>(temp: &'a mut Option<Client>, main: &'a mut Client) -> &'a mut Client {
    match temp {
        Some(client) => client,
        None => main,
    }
}

impl App {
    /// Attaches to another session. Fails, leaving the current session in place, when it is not running.
    pub async fn switch_session(&mut self, name: &str) -> Result<(), String> {
        if name == self.session {
            return Ok(());
        }
        let endpoint = ipc::endpoint_in(&self.socket_dir, name);
        let not_running = || format!("session '{name}' is not running");
        let client = Client::connect(&endpoint)
            .await
            .map_err(|_| not_running())?;
        let sub = Client::connect(&endpoint)
            .await
            .map_err(|_| not_running())?;
        self.endpoint = endpoint;
        self.session = name.to_string();
        self.client = client;
        self.reset_view();
        self.subscribe(sub);
        self.refresh_nodes().await;
        Ok(())
    }

    /// Switches to the named session, reporting failure on the status row.
    pub async fn go_session(&mut self, name: &str) -> super::app::Flow {
        match self.switch_session(name).await {
            Ok(()) => super::app::Flow::Continue,
            Err(e) => self.say(e),
        }
    }

    /// Clears per-session view state so the next frame reloads panes from the new session.
    fn reset_view(&mut self) {
        self.vt.clear();
        self.pane_dims.clear();
        self.expected.clear();
        self.layout.dirty = true;
        self.layout.raw.clear();
        self.zoomed = None;
        self.mode = Mode::Normal;
        self.parked_copy.clear();
        self.nodes.clear();
        self.focus_pane = None;
        self.active_pane.clear();
        self.split_counter = 1;
    }

    /// Resolves an optional `[session:]pane` target to the pane name and, for another session,
    /// a temporary connection to that session. Without a target the active pane is used.
    pub async fn resolve_target(
        &self,
        target: Option<&str>,
    ) -> Result<(Option<Client>, String), String> {
        let raw = target
            .filter(|t| !t.is_empty())
            .unwrap_or(self.active_pane.as_str());
        let (session, pane) = split_target(raw);
        if pane.is_empty() {
            return Err("no target pane".to_string());
        }
        match session.filter(|s| *s != self.session.as_str()) {
            None => Ok((None, pane.to_string())),
            Some(other) => {
                let endpoint = ipc::endpoint_in(&self.socket_dir, other);
                let client = Client::connect(&endpoint)
                    .await
                    .map_err(|_| format!("session '{other}' is not running"))?;
                Ok((Some(client), pane.to_string()))
            }
        }
    }
}
