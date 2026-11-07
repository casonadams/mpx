//! Session environment and session commands.

use super::{Tx, session_req, unavailable};
use crate::protocol::SessionRequest;
use crate::tui::command::{EnvCommand, SessionCommand};

pub(super) async fn environment(tx: &Tx, command: EnvCommand) -> Result<(), String> {
    let req = match command {
        EnvCommand::SetEnvironment { name, change } => {
            SessionRequest::SetEnvironment { name, change }
        }
        EnvCommand::ShowEnvironment { name } => SessionRequest::ShowEnvironment { name },
    };
    session_req(tx, req).await
}

/// Session commands the daemon carries out: renaming. Switching clients between sessions is not
/// available in a hook.
pub(super) async fn session(tx: &Tx, command: SessionCommand) -> Result<(), String> {
    match command {
        SessionCommand::RenameSession { name } => {
            session_req(tx, SessionRequest::Rename { name }).await
        }
        _ => Err(unavailable()),
    }
}
