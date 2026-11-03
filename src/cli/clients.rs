//! `list-clients`, `detach-client`, `refresh-client` and `switch-client` from the command line. Each
//! finds the attached clients across the running sessions and asks the daemon of the session the
//! client is attached to.
use crate::client::Client;
use crate::clients::{self, ClientRow, Size};
use crate::cmdline::join_words;
use crate::ipc;
use crate::protocol::ClientTarget;

use super::Commands;
use super::resolve::Resolved;

/// Runs one of the client commands (`list-clients`, `detach-client`, `refresh-client`,
/// `switch-client`).
pub async fn run(cmd: Commands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        Commands::ListClients { target, format } => {
            list(r, target.as_deref(), format.as_deref()).await
        }
        Commands::DetachClient { target, all_others } => {
            detach(r, target.as_deref(), all_others).await
        }
        Commands::RefreshClient { target, size, .. } => {
            refresh(r, target.as_deref(), size.as_deref()).await
        }
        Commands::SwitchClient {
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
        _ => unreachable!("sessions::dispatch routes only client commands here"),
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
