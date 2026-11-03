//! Session switching and `[session:]pane` target resolution.
use super::app::{App, Mode};
use super::panes::neighbor;
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
    let idx = names.iter().position(|n| n == current).unwrap_or(0);
    neighbor(names, idx, next).cloned()
}

/// Picks the temporary connection when one was opened, otherwise the attached client.
pub fn pick<'a>(temp: &'a mut Option<Client>, main: &'a mut Client) -> &'a mut Client {
    match temp {
        Some(client) => client,
        None => main,
    }
}

/// Whether the session behind `client` has a pane to show; one without has nothing to attach to,
/// and the TUI would end on its first frame.
async fn shows_panes(client: &mut Client) -> bool {
    client.layout(80, 24).await.is_ok_and(|(panes, ..)| !panes.is_empty())
}

/// The running session `name` refers to. A session's name is the one its daemon reports, which
/// `rename-session` changes without moving the socket, so a socket called `name` may belong to
/// some other session. Of the daemons that report `name`, one with panes wins, then the one whose
/// socket is called `name`.
pub fn pick_session<'a>(
    infos: &'a [crate::protocol::SessionInfo],
    name: &str,
) -> Result<&'a crate::protocol::SessionInfo, String> {
    let named: Vec<_> = infos.iter().filter(|i| i.name == name).collect();
    if named.is_empty() {
        return Err(format!("session '{name}' is not running"));
    }
    let socket_is_name = |i: &&crate::protocol::SessionInfo| {
        std::path::Path::new(&i.endpoint)
            .file_stem()
            .is_some_and(|stem| stem == name)
    };
    named
        .iter()
        .copied()
        .filter(|i| i.panes > 0)
        .max_by_key(|i| socket_is_name(i))
        .ok_or_else(|| format!("session '{name}' has no panes"))
}

impl App {
    /// Attaches to another session by name. Fails, leaving the current session in place, when it
    /// is not running or has no panes.
    pub async fn switch_session(&mut self, name: &str) -> Result<(), String> {
        if name == self.session {
            return Ok(());
        }
        let infos = ipc::session_infos(&self.socket_dir).await;
        let target = pick_session(&infos, name)?;
        self.attach_endpoint(&target.endpoint, &target.name).await
    }

    /// Attaches to the session whose daemon listens on `endpoint`, known as `name`.
    pub async fn attach_endpoint(&mut self, endpoint: &str, name: &str) -> Result<(), String> {
        let not_running = || format!("session '{name}' is not running");
        let mut client = Client::connect(endpoint).await.map_err(|_| not_running())?;
        if !shows_panes(&mut client).await {
            return Err(format!("session '{name}' has no panes"));
        }
        let sub = Client::connect(endpoint).await.map_err(|_| not_running())?;
        if self.session != name {
            self.previous_session = Some(self.session.clone());
        }
        self.endpoint = endpoint.to_string();
        self.session = name.to_string();
        self.terminal.session = name.to_string();
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
