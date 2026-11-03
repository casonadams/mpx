//! The session's attached clients: the subscribers that are terminals, and what requests do to them.
//! A client is registered by its session-wide subscription and gone when that subscription ends.
use crate::node::now_ms;
use crate::protocol::{ClientInfo, ClientTarget, StreamEvent};

use super::session::Subscriber;

/// The live clients of `session`, in the order they attached.
pub(super) fn list(subs: &[Subscriber], session: &str) -> Vec<ClientInfo> {
    live(subs)
        .into_iter()
        .map(|info| ClientInfo {
            session: session.to_string(),
            ..info
        })
        .collect()
}

/// The live clients as registered (the session name is filled in by the caller).
pub(super) fn live(subs: &[Subscriber]) -> Vec<ClientInfo> {
    let mut clients = Vec::new();
    for s in subs {
        if let Some(client) = &s.client
            && !s.sink.is_closed()
        {
            clients.push(client.clone());
        }
    }
    clients
}

/// Whether a client's subscription has ended since it attached.
pub(super) fn gone(subs: &[Subscriber]) -> bool {
    subs.iter()
        .any(|s| s.client.is_some() && s.sink.is_closed())
}

/// Detaches the clients `target` selects: each is sent `Detached` and its subscription ends.
/// Returns whether any client was detached. A named client that is not attached is an error.
pub(super) fn detach(subs: &mut Vec<Subscriber>, target: &ClientTarget) -> Result<bool, String> {
    if let ClientTarget::Named(name) = target
        && !subs.iter().any(|s| is_client(s, name))
    {
        return Err(not_found(name));
    }
    Ok(retain_detaching(subs, |name| match target {
        ClientTarget::Named(n) => n == name,
        ClientTarget::Others(keep) => keep != name,
        ClientTarget::All => true,
    }))
}

/// Detaches every client except the one called `keep`, as `attach -d` does for the new client.
/// Returns the parent processes of the clients detached, for `hang_up`.
pub(super) fn detach_others(subs: &mut Vec<Subscriber>, keep: &str) -> Vec<u32> {
    let parents = subs
        .iter()
        .filter(|s| !s.sink.is_closed())
        .filter_map(|s| s.client.as_ref())
        .filter(|c| c.name != keep)
        .filter_map(|c| c.parent_pid)
        .collect();
    retain_detaching(subs, |name| keep != name);
    parents
}

/// Sends SIGHUP to each of `pids`, the parents of clients `attach -x` detached. Unix only: elsewhere
/// the clients are detached but nothing is signalled. Pids 0 and 1 are never signalled.
#[cfg(unix)]
pub(super) fn hang_up(pids: &[u32]) {
    for &pid in pids {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            continue;
        };
        if pid > 1 {
            // SAFETY: kill(2) only sends a signal; a pid that has gone fails with ESRCH.
            unsafe { libc::kill(pid, libc::SIGHUP) };
        }
    }
}

#[cfg(not(unix))]
pub(super) fn hang_up(_pids: &[u32]) {}

fn retain_detaching(subs: &mut Vec<Subscriber>, selected: impl Fn(&str) -> bool) -> bool {
    let mut detached = false;
    subs.retain_mut(|s| {
        let Some(name) = s.client.as_ref().map(|c| c.name.as_str()) else {
            return true;
        };
        if !selected(name) {
            return true;
        }
        detached = true;
        let _ = s.sink.push(StreamEvent::Detached);
        false
    });
    detached
}

/// Marks the client `name` as the most recently active one.
pub(super) fn touch(subs: &mut [Subscriber], name: &str) -> Result<(), String> {
    client_mut(subs, name)?.activity_ms = now_ms();
    Ok(())
}

/// Sets the client `name`'s size. Returns whether it changed.
pub(super) fn resize(
    subs: &mut [Subscriber],
    name: &str,
    width: u16,
    height: u16,
) -> Result<bool, String> {
    let info = client_mut(subs, name)?;
    let changed = (info.width, info.height) != (width, height);
    info.width = width;
    info.height = height;
    Ok(changed)
}

/// Redraws the client `name`, after giving it `size` when one is asked for. Returns whether its
/// size changed.
pub(super) fn refresh(
    subs: &mut [Subscriber],
    name: &str,
    size: Option<(u16, u16)>,
) -> Result<bool, String> {
    let changed = match size {
        Some((width, height)) => resize(subs, name, width, height)?,
        None => false,
    };
    send(subs, name, StreamEvent::Refresh)?;
    Ok(changed)
}

/// Sends `event` to the client `name` alone.
pub(super) fn send(subs: &mut [Subscriber], name: &str, event: StreamEvent) -> Result<(), String> {
    let sub = subs
        .iter_mut()
        .find(|s| is_client(s, name))
        .ok_or_else(|| not_found(name))?;
    let _ = sub.sink.push(event);
    Ok(())
}

fn is_client(sub: &Subscriber, name: &str) -> bool {
    !sub.sink.is_closed() && sub.client.as_ref().is_some_and(|c| c.name == name)
}

fn client_mut<'a>(subs: &'a mut [Subscriber], name: &str) -> Result<&'a mut ClientInfo, String> {
    subs.iter_mut()
        .filter(|s| !s.sink.is_closed())
        .find_map(|s| s.client.as_mut().filter(|c| c.name == name))
        .ok_or_else(|| not_found(name))
}

fn not_found(name: &str) -> String {
    format!("can't find client: {name}")
}
