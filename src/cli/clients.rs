//! `list-clients`, `detach-client`, `refresh-client` and `switch-client` from the command line. Each
//! finds the attached clients across the running sessions and asks the daemon of the session the
//! client is attached to.
use crate::client::Client;
use crate::clients::{self, ClientRow, Size};
use crate::cmdline::join_words;
use crate::ipc;
use crate::protocol::ClientTarget;

use clap::Subcommand;

use super::resolve::Resolved;

/// The client commands of the CLI, flattened into `Commands`.
#[derive(Subcommand, Debug)]
pub enum ClientCommands {
    /// List attached clients across sessions, or one session's (tmux list-clients)
    #[command(name = "list-clients", alias = "lsc")]
    ListClients {
        /// Only the clients of this session (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Format of each line (tmux -F)
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
    },

    /// Detach a client (tmux detach-client): the most recently active one unless -t names it
    #[command(name = "detach-client")]
    DetachClient {
        /// Client to detach (tmux -t), as list-clients names it
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Detach the other clients of the target's session instead (tmux -a)
        #[arg(short = 'a')]
        all_others: bool,
    },

    /// Redraw a client, giving it a size with -C (tmux refresh-client)
    #[command(name = "refresh-client")]
    RefreshClient {
        /// Client to refresh (tmux -t); every client when omitted
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Size of the client in WIDTHxHEIGHT (tmux -C)
        #[arg(short = 'C', long = "size")]
        size: Option<String>,

        /// Status line refresh (tmux -S); a full redraw is done either way
        #[arg(short = 'S')]
        status: bool,
    },

    /// Switch a client to another session (tmux switch-client)
    #[command(name = "switch-client", alias = "switchc")]
    SwitchClient {
        /// Client to switch (tmux -c); the most recently active one when omitted
        #[arg(short = 'c', long = "client")]
        client: Option<String>,

        /// Session to switch to (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// The session switched from last (tmux -l)
        #[arg(short = 'l', conflicts_with_all = ["target", "next", "prev"])]
        last: bool,

        /// The next session (tmux -n)
        #[arg(short = 'n', conflicts_with_all = ["target", "last", "prev"])]
        next: bool,

        /// The previous session (tmux -p)
        #[arg(short = 'p', conflicts_with_all = ["target", "last", "next"])]
        prev: bool,
    },
}

/// Runs one of the client commands.
pub async fn run(cmd: ClientCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        ClientCommands::ListClients { target, format } => {
            list(r, target.as_deref(), format.as_deref()).await
        }
        ClientCommands::DetachClient { target, all_others } => {
            detach(r, target.as_deref(), all_others).await
        }
        ClientCommands::RefreshClient { target, size, .. } => {
            refresh(r, target.as_deref(), size.as_deref()).await
        }
        ClientCommands::SwitchClient {
            client,
            target,
            last,
            next,
            prev,
        } => {
            let step = [(last, "-l"), (next, "-n"), (prev, "-p")]
                .into_iter()
                .find(|(set, _)| *set)
                .map(|(_, flag)| flag);
            switch(r, client.as_deref(), target.as_deref(), step).await
        }
    }
}

/// Client rows that match `target` (a client name), or every client when `None`.
fn rows_for(rows: &[ClientRow], target: Option<&str>) -> Result<Vec<ClientRow>, String> {
    match target {
        None => Ok(rows.to_vec()),
        Some(name) => clients::find_row(rows, name)
            .cloned()
            .map(|row| vec![row])
            .ok_or_else(|| format!("can't find client: {name}")),
    }
}

/// The client `target` names, or the most recently active one.
fn chosen<'a>(rows: &'a [ClientRow], target: Option<&str>) -> Result<&'a ClientRow, String> {
    match target {
        Some(name) => {
            clients::find_row(rows, name).ok_or_else(|| format!("can't find client: {name}"))
        }
        None => clients::latest_row(rows).ok_or_else(|| "no clients attached".to_string()),
    }
}

/// `list-clients [-t session] [-F format]`: one line per client.
async fn list(r: &Resolved, session: Option<&str>, format: Option<&str>) -> Result<i32, String> {
    let mut rows = clients::attached_clients(&r.socket_dir).await;
    if let Some(name) = session {
        rows.retain(|row| row.info.session == name);
    }
    for line in clients::client_lines(&rows, format.unwrap_or(clients::DEFAULT_CLIENT_FORMAT)) {
        println!("{line}");
    }
    Ok(0)
}

/// `detach-client [-a] [-t client]`: detaches the client, or with `all_others` the other clients
/// of its session.
async fn detach(r: &Resolved, target: Option<&str>, all_others: bool) -> Result<i32, String> {
    let rows = clients::attached_clients(&r.socket_dir).await;
    let row = chosen(&rows, target)?;
    let request = if all_others {
        ClientTarget::Others(row.info.name.clone())
    } else {
        ClientTarget::Named(row.info.name.clone())
    };
    let mut client = Client::connect(&row.endpoint).await?;
    client.detach_clients(request).await?;
    Ok(0)
}

/// `refresh-client [-t client] [-C WxH]`: redraws the client (every client when `target` is
/// `None`), giving it `size` first when one is set.
async fn refresh(r: &Resolved, target: Option<&str>, size: Option<&str>) -> Result<i32, String> {
    let size: Option<(u16, u16)> = size
        .map(str::parse::<Size>)
        .transpose()?
        .map(|s| (s.width, s.height));
    let rows = clients::attached_clients(&r.socket_dir).await;
    for row in rows_for(&rows, target)? {
        let mut client = Client::connect(&row.endpoint).await?;
        client.refresh_client(&row.info.name, size).await?;
    }
    Ok(0)
}

/// `switch-client [-c client] (-t session | -l | -n | -p)`: runs the switch in the client.
async fn switch(
    r: &Resolved,
    client: Option<&str>,
    session: Option<&str>,
    step: Option<&str>,
) -> Result<i32, String> {
    let words: Vec<String> = match (session, step) {
        (Some(name), None) => {
            let running = ipc::session_infos(&r.socket_dir).await;
            if !running.iter().any(|info| info.name == name) {
                return Err(format!("session '{name}' is not running"));
            }
            vec!["switch-client".into(), "-t".into(), name.into()]
        }
        (None, Some(flag)) => vec!["switch-client".into(), flag.into()],
        _ => return Err("switch-client: give -t SESSION, or one of -l, -n, -p".into()),
    };
    let rows = clients::attached_clients(&r.socket_dir).await;
    let row = chosen(&rows, client)?;
    let mut connection = Client::connect(&row.endpoint).await?;
    connection
        .client_command(&row.info.name, &join_words(&words))
        .await?;
    Ok(0)
}
