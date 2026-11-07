use super::Client;
use crate::protocol::{ClientInfo, ClientRequest, ClientTarget, Request, Response};

impl Client {
    /// The clients attached to this session.
    pub async fn list_clients(&mut self) -> Result<Vec<ClientInfo>, String> {
        request!(self, "ListClients", Request::Client(ClientRequest::List), Response::Clients(clients) => clients)
    }

    /// Detaches the clients `target` selects.
    pub async fn detach_clients(&mut self, target: ClientTarget) -> Result<(), String> {
        let req = Request::Client(ClientRequest::Detach { target });
        self.call_ok("DetachClients", req).await
    }

    /// Redraws client `name`, first giving it `size` (cols, rows) when one is asked for.
    pub async fn refresh_client(
        &mut self,
        name: &str,
        size: Option<(u16, u16)>,
    ) -> Result<(), String> {
        let req = Request::Client(ClientRequest::Refresh {
            name: name.to_string(),
            size,
        });
        self.call_ok("RefreshClient", req).await
    }

    /// Records that client `name` gave input.
    pub async fn client_activity(&mut self, name: &str) -> Result<(), String> {
        let req = Request::Client(ClientRequest::Activity {
            name: name.to_string(),
        });
        self.call_ok("ClientActivity", req).await
    }

    /// Records client `name`'s terminal size.
    pub async fn client_resized(
        &mut self,
        name: &str,
        width: u16,
        height: u16,
    ) -> Result<(), String> {
        let req = Request::Client(ClientRequest::Resized {
            name: name.to_string(),
            width,
            height,
        });
        self.call_ok("ClientResized", req).await
    }

    /// Sends the command line `line` to client `name` to run there.
    pub async fn client_command(&mut self, name: &str, line: &str) -> Result<(), String> {
        let req = Request::Client(ClientRequest::Command {
            name: name.to_string(),
            line: line.to_string(),
        });
        self.call_ok("ClientCommand", req).await
    }
}
