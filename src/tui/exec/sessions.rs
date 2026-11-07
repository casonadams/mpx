//! Sessions, the session environment and the attached clients.
use crate::ipc;
use crate::tui::app::{App, Flow};
use crate::tui::command::{ClientCommand, EnvCommand, SessionCommand};
use crate::tui::overlay::Overlay;
use crate::tui::session::neighbor_session;

impl App {
    pub(super) async fn session_command(&mut self, cmd: SessionCommand) -> Flow {
        match cmd {
            SessionCommand::SwitchSession { name } => self.go_session(&name).await,
            SessionCommand::NextSession => self.step_session(true).await,
            SessionCommand::PrevSession => self.step_session(false).await,
            SessionCommand::LastSession => self.last_session().await,
            SessionCommand::RenameSession { name } => {
                let result = self.client.rename_session(&name).await;
                if result.is_ok() {
                    self.session = name;
                }
                self.settle(result)
            }
        }
    }

    pub(super) async fn kill_server(&mut self) -> Flow {
        let _ = self.client.shutdown().await;
        Flow::Quit
    }

    /// Switches to the session that was current before this one, if there was one.
    async fn last_session(&mut self) -> Flow {
        match self.attachment.previous_session.clone() {
            Some(name) => self.go_session(&name).await,
            None => self.say("no last session"),
        }
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

    pub(super) async fn environment_command(&mut self, cmd: EnvCommand) -> Flow {
        match cmd {
            EnvCommand::SetEnvironment { name, change } => {
                let result = self.client.set_environment(&name, change).await;
                self.settle(result)
            }
            EnvCommand::ShowEnvironment { name } => {
                let lines = self
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
        }
    }

    pub(super) async fn client_command(&mut self, cmd: ClientCommand) -> Flow {
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

    /// `suspend-client`: the event loop stops the process once the command has run.
    fn suspend_client(&mut self) -> Flow {
        if cfg!(unix) {
            self.chrome.suspend_requested = true;
            Flow::Continue
        } else {
            self.say("suspend-client is not supported on this platform")
        }
    }
}
