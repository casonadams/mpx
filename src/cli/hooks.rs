use clap::Subcommand;

use super::resolve::Resolved;
use super::sessions::ensure_client;
use crate::cmdline::command_line;
use crate::protocol::{ChannelOp, Request, Response, ScriptRequest};

/// The daemon-side commands of the CLI, flattened into `Commands`.
#[derive(Subcommand, Debug)]
pub enum HookCommands {
    /// Run a command through the session's shell (tmux run-shell); formats in it are expanded for
    /// the target pane, and its output is printed
    #[command(name = "run-shell")]
    RunShell {
        /// Do not wait for the command; print nothing (tmux -b)
        #[arg(short = 'b')]
        background: bool,

        /// Pane whose formats are expanded and whose directory the command starts in (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Wait this many seconds before running (tmux -d)
        #[arg(short = 'd', value_name = "SECONDS")]
        delay: Option<f64>,

        /// Command to run, with its arguments
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        command: Vec<String>,
    },

    /// Run one command if a shell command succeeds, else another (tmux if-shell); `-F` tests a
    /// format instead, true when it is not empty or `0`. The chosen command runs in the session,
    /// as a hook's commands do.
    #[command(name = "if-shell")]
    IfShell {
        /// The condition is a format, not a shell command (tmux -F)
        #[arg(short = 'F')]
        format: bool,

        /// Pane the condition and the commands act on (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Shell command whose exit status decides the branch; with -F, a format
        condition: String,

        /// Command run when the condition passes
        then: String,

        /// Command run when the condition fails
        otherwise: Option<String>,
    },

    /// Wait for a signal on a named channel, or lock or unlock it (tmux wait-for)
    #[command(name = "wait-for")]
    WaitFor {
        /// Signal the channel, waking its waiters (tmux -S)
        #[arg(short = 'S', conflicts_with_all = ["lock", "unlock"])]
        signal: bool,

        /// Take the channel's lock, waiting until it is free (tmux -L)
        #[arg(short = 'L', conflicts_with = "unlock")]
        lock: bool,

        /// Release the channel's lock (tmux -U)
        #[arg(short = 'U')]
        unlock: bool,

        /// Channel name
        channel: String,
    },

    /// Set the command a hook runs on an event (tmux set-hook); `-a` appends, `-u` removes, `-R`
    /// runs the hook now
    #[command(name = "set-hook")]
    SetHook {
        /// Add the command after the hook's existing commands (tmux -a)
        #[arg(short = 'a', conflicts_with_all = ["unset", "run"])]
        append: bool,

        /// Remove the hook's commands (tmux -u)
        #[arg(short = 'u', conflicts_with = "run")]
        unset: bool,

        /// Run the hook's commands now (tmux -R)
        #[arg(short = 'R')]
        run: bool,

        /// Hook name, such as after-new-window or pane-exited
        hook: String,

        /// Command line the hook runs
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
        command: Vec<String>,
    },

    /// List every hook and its command (tmux show-hooks)
    #[command(name = "show-hooks")]
    ShowHooks,
}

/// Runs a daemon-side command of the CLI: `run-shell`, `if-shell`, `wait-for`, `set-hook` and
/// `show-hooks`.
pub async fn run(cmd: HookCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        HookCommands::RunShell {
            background,
            target,
            delay,
            command,
        } => run_shell(r, background, target.as_deref(), delay, &command.join(" ")).await,
        HookCommands::IfShell {
            format,
            target,
            condition,
            then,
            otherwise,
        } => {
            let branches = Branches {
                condition: &condition,
                then: &then,
                otherwise: otherwise.as_deref(),
            };
            if_shell(r, format, target.as_deref(), branches).await
        }
        HookCommands::WaitFor {
            signal,
            lock,
            unlock,
            channel,
        } => wait_for(r, (signal, lock, unlock), &channel).await,
        HookCommands::SetHook {
            append,
            unset,
            run,
            hook,
            command,
        } => set_hook(r, (append, unset, run), &hook, &command).await,
        HookCommands::ShowHooks => show_hooks(r).await,
    }
}

/// `run-shell`: formats expanded for the target, then the command run in the daemon; its output is
/// printed. A background run prints nothing.
async fn run_shell(
    r: &Resolved,
    background: bool,
    target: Option<&str>,
    delay: Option<f64>,
    command: &str,
) -> Result<i32, String> {
    let delay_ms = match delay {
        Some(secs) if secs.is_finite() && secs >= 0.0 => Some((secs * 1000.0).round() as u64),
        Some(secs) => return Err(format!("run-shell: invalid delay {secs}")),
        None => None,
    };
    let mut client = ensure_client(r).await?;
    let req = Request::Script(ScriptRequest::Format {
        fmt: command.to_string(),
        target: target.map(str::to_string),
        hook: None,
    });
    let expanded = match client.call(req).await {
        Ok(Response::Text(text)) => text,
        Ok(Response::Error(e)) | Err(e) => return Err(format!("run-shell error: {e}")),
        Ok(other) => return Err(format!("run-shell error: unexpected response {other:?}")),
    };
    let shell = client
        .run_shell(&expanded, target, background, delay_ms)
        .await
        .map_err(|e| format!("run-shell error: {e}"))?;
    if let Some(shell) = shell {
        print!("{}", shell.output);
    }
    Ok(0)
}

/// The condition and branches of an `if-shell`.
struct Branches<'a> {
    condition: &'a str,
    then: &'a str,
    otherwise: Option<&'a str>,
}

/// `if-shell [-F] [-t pane] condition then [otherwise]`: the line goes to the daemon, which decides
/// the branch and runs it.
async fn if_shell(
    r: &Resolved,
    format: bool,
    target: Option<&str>,
    branches: Branches<'_>,
) -> Result<i32, String> {
    let mut words = vec!["if-shell".to_string()];
    if format {
        words.push("-F".to_string());
    }
    if let Some(target) = target {
        words.extend(["-t".to_string(), target.to_string()]);
    }
    words.push(branches.condition.to_string());
    words.push(branches.then.to_string());
    words.extend(branches.otherwise.map(str::to_string));
    let line = command_line(&words);
    let mut client = ensure_client(r).await?;
    client
        .call_ok("RunCommand", Request::Script(ScriptRequest::RunCommand { line }))
        .await
        .map_err(|e| format!("if-shell error: {e}"))?;
    Ok(0)
}

/// `wait-for`: a signal wakes the channel's waiters; `-L` and `-U` take and release its lock.
async fn wait_for(
    r: &Resolved,
    (signal, lock, unlock): (bool, bool, bool),
    channel: &str,
) -> Result<i32, String> {
    let op = match (signal, lock, unlock) {
        (false, false, false) => ChannelOp::Wait,
        (true, _, _) => ChannelOp::Signal,
        (_, true, _) => ChannelOp::Lock,
        (_, _, true) => ChannelOp::Unlock,
    };
    let mut client = ensure_client(r).await?;
    let req = Request::Script(ScriptRequest::WaitFor {
        channel: channel.to_string(),
        op,
    });
    client
        .call_ok("WaitFor", req)
        .await
        .map_err(|e| format!("wait-for error: {e}"))?;
    Ok(0)
}

/// `set-hook`: sets, removes or runs the commands of a hook.
async fn set_hook(
    r: &Resolved,
    (append, unset, run): (bool, bool, bool),
    hook: &str,
    command: &[String],
) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let (op, req) = if run {
        if !command.is_empty() {
            return Err("set-hook: -R takes no command".to_string());
        }
        ("RunHook", Request::Script(ScriptRequest::RunHook { name: hook.to_string() }))
    } else if unset {
        (
            "SetHook",
            Request::Script(ScriptRequest::SetHook {
                name: hook.to_string(),
                command: None,
                append: false,
            }),
        )
    } else if command.is_empty() {
        return Err(format!("set-hook: need a command for hook {hook}"));
    } else {
        (
            "SetHook",
            Request::Script(ScriptRequest::SetHook {
                name: hook.to_string(),
                command: Some(command_line(command)),
                append,
            }),
        )
    };
    client
        .call_ok(op, req)
        .await
        .map_err(|e| format!("set-hook error: {e}"))?;
    Ok(0)
}

/// `show-hooks`: every hook command, one line each.
async fn show_hooks(r: &Resolved) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    match client.call(Request::Script(ScriptRequest::ShowHooks)).await {
        Ok(Response::Hooks(hooks)) => {
            for (name, command) in hooks {
                println!("{name} {command}");
            }
            Ok(0)
        }
        Ok(Response::Error(e)) | Err(e) => Err(format!("show-hooks error: {e}")),
        Ok(other) => Err(format!("show-hooks error: unexpected response {other:?}")),
    }
}
