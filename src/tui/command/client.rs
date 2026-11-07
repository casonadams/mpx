//! Client commands: listing, detaching and refreshing the terminals attached to the session.
use super::model::{ClientCommand, Command};
use super::opts::{Operands, Spec, with_opts};

const DETACH_CLIENT: Spec = Spec::flags(&["-a"], &["-t"]);
const LIST_CLIENTS: Spec = Spec::flags(&[], &["-F", "-t"]);
const REFRESH_CLIENT: Spec = Spec::flags(&["-S"], &["-C", "-t"]);

/// `detach-client [-a] [-t client]`: plain `detach-client` detaches this client.
pub(super) fn detach_client(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, DETACH_CLIENT, Operands::Refuse, |o| {
        let target = o.string("-t");
        let all_others = o.has("-a");
        if target.is_none() && !all_others {
            return Ok(Command::Detach);
        }
        Ok(Command::Client(ClientCommand::DetachClients {
            target,
            all_others,
        }))
    })
}

/// `list-clients [-F format] [-t session]`.
pub(super) fn list_clients(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, LIST_CLIENTS, Operands::Refuse, |o| {
        Ok(Command::Client(ClientCommand::ListClients {
            target: o.string("-t"),
            format: o.string("-F"),
        }))
    })
}

/// `refresh-client [-S] [-C WxH] [-t client]`. The whole client is redrawn, which includes its
/// status line, so `-S` changes nothing.
pub(super) fn refresh_client(name: &str, args: &[String]) -> Result<Command, String> {
    with_opts(name, args, REFRESH_CLIENT, Operands::Refuse, |o| {
        let size = o
            .value("-C")
            .map(str::parse::<crate::clients::Size>)
            .transpose()
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(Command::Client(ClientCommand::RefreshClient {
            target: o.string("-t"),
            size: size.map(|s| (s.width, s.height)),
        }))
    })
}
