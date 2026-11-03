//! The session environment: variables that panes started afterwards inherit (tmux set-environment).
use clap::Subcommand;

use super::resolve::Resolved;
use super::sessions::{ensure_client, target_resolved};
use crate::protocol::EnvChange;

#[derive(Subcommand, Debug)]
pub enum EnvCommands {
    /// Set a variable of the session environment for panes started afterwards (tmux set-environment)
    #[command(name = "set-environment", alias = "setenv")]
    SetEnvironment {
        /// Session (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Accepted for tmux compatibility: mpx has one environment per session, this one
        #[arg(short = 'g')]
        global: bool,

        /// Remove the variable from the session environment
        #[arg(short = 'u', conflicts_with_all = ["remove", "value"])]
        unset: bool,

        /// Keep the variable out of new panes, even when mpx inherited it
        #[arg(short = 'r')]
        remove: bool,

        name: String,
        #[arg(allow_hyphen_values = true)]
        value: Option<String>,
    },

    /// Show the session environment, or one variable of it (tmux show-environment)
    #[command(name = "show-environment", alias = "showenv")]
    ShowEnvironment {
        /// Session (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Accepted for tmux compatibility (see set-environment -g)
        #[arg(short = 'g')]
        global: bool,

        name: Option<String>,
    },
}

pub async fn run(cmd: EnvCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        EnvCommands::SetEnvironment {
            target,
            global: _,
            unset,
            remove,
            name,
            value,
        } => {
            let change = match (unset, remove, value) {
                (true, _, _) => EnvChange::Unset,
                (_, true, _) => EnvChange::Remove,
                (_, _, Some(value)) => EnvChange::Set(value),
                (_, _, None) => return Err(format!("set-environment: no value given for {name}")),
            };
            let mut client = ensure_client(&session_target(r, target.as_deref())).await?;
            client
                .set_environment(&name, change)
                .await
                .map_err(|e| format!("set-environment: {e}"))?;
            Ok(0)
        }
        EnvCommands::ShowEnvironment {
            target,
            global: _,
            name,
        } => {
            let mut client = ensure_client(&session_target(r, target.as_deref())).await?;
            let entries = client
                .show_environment(name.as_deref())
                .await
                .map_err(|e| format!("show-environment: {e}"))?;
            for entry in entries {
                match entry.value {
                    Some(value) => println!("{}={value}", entry.name),
                    None => println!("-{}", entry.name),
                }
            }
            Ok(0)
        }
    }
}

fn session_target(r: &Resolved, target: Option<&str>) -> Resolved {
    match target {
        Some(target) => target_resolved(r, target.rsplit(':').next().unwrap_or(target)),
        None => r.clone(),
    }
}
