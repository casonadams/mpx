//! The TUI as one of its session's clients: the size and activity it reports to the daemon, the
//! clients `list-clients`, `detach-client` and `refresh-client` act on, and what a session ending
//! means for it.
use crate::client::Client;
use crate::clients::{self, DEFAULT_CLIENT_FORMAT};
use crate::ipc;
use crate::protocol::ClientTarget;

use super::app::{App, Flow};
use super::command;
use super::overlay::Overlay;

impl App {
    /// Tells the daemon this client gave input: the `latest` window-size policy follows it.
    pub async fn touch_client(&mut self) {
        let name = self.terminal.name.clone();
        let result = self.client.client_activity(&name).await;
        self.report("client activity", result);
    }

    /// Tells the daemon the size of the area this client shows the window in, when it changed.
    pub async fn report_size(&mut self, width: u16, height: u16) {
        if (self.terminal.width, self.terminal.height) == (width, height) {
            return;
        }
        let name = self.terminal.name.clone();
        let result = self.client.client_resized(&name, width, height).await;
        if result.is_ok() {
            self.terminal.width = width;
            self.terminal.height = height;
        }
        self.report("resize", result);
    }

    /// Counts the clients attached to this session, for `session_attached`.
    pub async fn refresh_attached(&mut self) {
        if let Ok(clients) = self.client.list_clients().await {
            self.attached = clients.len();
        }
    }

    /// `list-clients [-t session] [-F format]`: the clients of `target` (of every session without
    /// one), a line each, in an overlay.
    pub async fn list_clients(&mut self, target: Option<String>, format: Option<String>) -> Flow {
        let mut rows = clients::attached_clients(&self.socket_dir).await;
        if let Some(session) = target {
            rows.retain(|r| r.info.session == session);
        }
        if rows.is_empty() {
            return self.say("no clients");
        }
        let format = format.as_deref().unwrap_or(DEFAULT_CLIENT_FORMAT);
        let lines = clients::client_lines(&rows, format);
        self.open_overlay(Overlay::text("list-clients", lines))
            .await
    }

    /// `detach-client [-a] [-t client]`: detaches `target`, or with `all_others` the other clients
    /// of its session. Detaching this client ends the TUI.
    pub async fn detach_clients(&mut self, target: Option<String>, all_others: bool) -> Flow {
        let own = self.terminal.name.clone();
        let keep = target.clone().unwrap_or_else(|| own.clone());
        if !all_others && (target.is_none() || keep == own) {
            return Flow::Quit;
        }
        let request = if all_others {
            ClientTarget::Others(keep.clone())
        } else {
            ClientTarget::Named(keep.clone())
        };
        let result = match self.endpoint_of(&keep).await {
            Ok(endpoint) => match Client::connect(&endpoint).await {
                Ok(mut client) => client.detach_clients(request).await,
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        self.settle(result)
    }

    /// `refresh-client [-t client] [-C WxH]`: redraws `target` (this client when `None`), giving it
    /// `size` first when one is set.
    pub async fn refresh_client(
        &mut self,
        target: Option<String>,
        size: Option<(u16, u16)>,
    ) -> Flow {
        let name = target.unwrap_or_else(|| self.terminal.name.clone());
        let result = match self.endpoint_of(&name).await {
            Ok(endpoint) => match Client::connect(&endpoint).await {
                Ok(mut client) => client.refresh_client(&name, size).await,
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        self.settle(result)
    }

    /// Runs a command line another client of the session forwarded here (`switch-client` run from
    /// the command line).
    pub async fn run_forwarded(&mut self, line: &str) -> Flow {
        match command::parse(line) {
            Ok(cmd) => self.run(cmd).await,
            Err(e) => self.say(e),
        }
    }

    /// The daemon of the session the client called `name` is attached to.
    async fn endpoint_of(&self, name: &str) -> Result<String, String> {
        if name == self.terminal.name {
            return Ok(self.endpoint.clone());
        }
        let rows = clients::attached_clients(&self.socket_dir).await;
        clients::find_row(&rows, name)
            .map(|row| row.endpoint.clone())
            .ok_or_else(|| format!("can't find client: {name}"))
    }

    /// The session this client was attached to ends. With `detach-on-destroy off` the client moves
    /// to another running session with panes and returns true with its frame ready; otherwise it
    /// is detached and returns false.
    pub async fn switch_after_destroy(&mut self) -> bool {
        if self.config.session.detach_on_destroy {
            return false;
        }
        let infos = ipc::session_infos(&self.socket_dir).await;
        let Some(next) = infos
            .into_iter()
            .find(|info| info.name != self.session && info.panes > 0)
        else {
            return false;
        };
        if self
            .attach_endpoint(&next.endpoint, &next.name)
            .await
            .is_err()
        {
            return false;
        }
        self.prepare_frame().await
    }
}
