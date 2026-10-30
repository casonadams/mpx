//! Which mpx session a tmux invocation talks to. A tmux server holds many sessions; mpx runs one
//! daemon per session, so a server is a directory of session sockets. `-L name` and `-S path`
//! each select their own directory next to the default one.
use std::path::PathBuf;

use super::args::Invocation;
use super::exec::EnvTarget;
use crate::{cli::DEFAULT_SESSION, ipc, store};

/// Relative location of the `-L` / `-S` server, under both the socket and the state directory;
/// `None` for the default server (`-L default` is the same as no flag).
pub fn server_key(name: Option<&str>, path: Option<&str>) -> Option<PathBuf> {
    let (kind, value) = match (path, name) {
        (Some(path), _) => ("path", path),
        (None, Some(name)) if name != "default" => ("name", name),
        _ => return None,
    };
    Some(
        PathBuf::from("servers")
            .join(kind)
            .join(store::component(value)),
    )
}

/// The session a command without `-t` talks to on a server: the lowest-numbered live one, else the
/// default session. Numbers sort before names, in numeric order.
pub fn default_session(live: &[String]) -> String {
    live.iter()
        .min_by_key(|name| match name.parse::<u64>() {
            Ok(n) => (0, n, String::new()),
            Err(_) => (1, 0, (*name).clone()),
        })
        .cloned()
        .unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

impl EnvTarget {
    /// Target of `inv`: its `-L` / `-S` server when given, else the environment's, and the session
    /// named by a `session:` prefix of `-t` when that session is running.
    pub fn for_invocation(inv: &Invocation) -> Self {
        let mut target = match server_key(inv.socket_name.as_deref(), inv.socket_path.as_deref()) {
            Some(key) => {
                let dir = ipc::socket_dir().join(key);
                let live: Vec<String> = ipc::list_sessions(&dir)
                    .into_iter()
                    .map(|s| s.name)
                    .collect();
                let session = default_session(&live);
                Self {
                    endpoint: ipc::endpoint_in(&dir, &session),
                    session,
                }
            }
            None => {
                let mut target = Self::from_env();
                target.fall_back_to_live();
                target
            }
        };
        target.retarget(inv.value('t'));
        target
    }

    /// Without `$TMUX`, `$MPX_ENDPOINT` or `$MPX_SESSION` naming a session, a command that finds the
    /// default session down talks to the lowest-numbered running one, as tmux's single server would.
    fn fall_back_to_live(&mut self) {
        let named = ["TMUX", "MPX_ENDPOINT", "MPX_SESSION"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()));
        if named {
            return;
        }
        let dir = self.socket_dir();
        let live: Vec<String> = ipc::list_sessions(&dir).into_iter().map(|s| s.name).collect();
        if live.is_empty() || live.contains(&self.session) {
            return;
        }
        self.session = default_session(&live);
        self.endpoint = ipc::endpoint_in(&dir, &self.session);
    }

    /// Moves to the session a `session:...` target names, when it is running.
    pub fn retarget(&mut self, target: Option<&str>) {
        let Some(name) = target
            .and_then(|t| t.split_once(':'))
            .map(|(session, _)| session)
            .filter(|s| !s.is_empty() && *s != self.session)
        else {
            return;
        };
        let dir = self.socket_dir();
        if ipc::list_sessions(&dir).iter().any(|s| s.name == name) {
            self.endpoint = ipc::endpoint_in(&dir, name);
            self.session = name.to_string();
        }
    }

    /// State directory for sessions of a `-L` / `-S` server; `None` on the default server.
    pub fn state_root(inv: &Invocation) -> Option<PathBuf> {
        server_key(inv.socket_name.as_deref(), inv.socket_path.as_deref())
            .map(|key| store::state_dir().join(key))
    }
}
