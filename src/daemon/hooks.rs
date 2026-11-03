//! Hooks: the commands the daemon runs when an event happens (`set-hook`). A hook keeps the text
//! it was set with and parses it when it runs, with the config's command aliases.
//!
//! Hook commands run in the daemon through the same command language as the TUI's `:` prompt and
//! bindings, but only the commands that the daemon can carry out are available: window, pane and
//! layout selection, `run-shell`, `if-shell`, `wait-for`, and hook and alias settings. Commands
//! that need the terminal (copy mode, prompts, overlays, client commands) fail with an error.
//! Commands a hook runs do not fire hooks themselves, so a hook cannot loop.
use std::collections::BTreeMap;
use std::pin::Pin;

use tokio::sync::mpsc;
use tracing::warn;

use super::SESSION_GONE;
use super::session::{self, HookMsg, Origin, SessionMsg};
use super::shell;
use crate::node::pane::ask_actor;
use crate::protocol::{NewWindowOpts, Request, Response, SpawnOpts, SplitOpts};
use crate::tui::command::{Command, ScriptCommand};

pub(super) type Tx = mpsc::UnboundedSender<SessionMsg>;

/// The commands of each hook name, in the order they were added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Hooks {
    commands: BTreeMap<String, Vec<String>>,
}

impl Hooks {
    /// Sets the commands of `name`: `append` adds one after the existing commands, else they are
    /// replaced. `None` removes the hook.
    pub fn set(&mut self, name: &str, command: Option<String>, append: bool) {
        match (command, append) {
            (None, _) => {
                self.commands.remove(name);
            }
            (Some(command), true) => self
                .commands
                .entry(name.to_string())
                .or_default()
                .push(command),
            (Some(command), false) => {
                self.commands.insert(name.to_string(), vec![command]);
            }
        }
    }

    /// The command lines of hook `name`, in order.
    pub fn commands(&self, name: &str) -> &[String] {
        self.commands.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every command line with its hook name, by name and then in order.
    pub fn list(&self) -> Vec<(String, String)> {
        self.commands
            .iter()
            .flat_map(|(name, lines)| lines.iter().map(move |line| (name.clone(), line.clone())))
            .collect()
    }
}

/// What a hook runs on: the event, the pane it happened in, and the config's window numbering.
#[derive(Debug, Clone)]
pub(super) struct Context {
    pub event: String,
    pub pane: Option<String>,
    pub base_index: usize,
}

/// Runs the commands of one event in a task of their own, so the session never waits for them.
pub(super) fn spawn(tx: Tx, ctx: Context, commands: Vec<Command>) {
    tokio::spawn(async move {
        for command in commands {
            if let Err(e) = run_command(&tx, &ctx, command).await {
                warn!("hook {}: {}", ctx.event, e);
            }
        }
    });
}

/// Runs one command as a hook runs its commands, and answers once it has run (`if-shell` from the CLI).
pub(super) async fn run_now(tx: Tx, ctx: Context, command: Command) -> Result<(), String> {
    run_command(&tx, &ctx, command).await
}

type Run<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

fn run_command<'a>(tx: &'a Tx, ctx: &'a Context, command: Command) -> Run<'a> {
    Box::pin(async move {
        match command {
            Command::Sequence(commands) => {
                for command in commands {
                    run_command(tx, ctx, command).await?;
                }
                Ok(())
            }
            Command::Script(script) => run_script(tx, ctx, script).await,
            Command::SplitWindow {
                side_by_side,
                argv,
                cwd,
                detach,
            } => {
                let opts = SplitOpts {
                    target: Some(pane(ctx)?),
                    spawn: spawn_opts(None, argv, cwd),
                    direction: if side_by_side {
                        crate::layout::Direction::Horizontal
                    } else {
                        crate::layout::Direction::Vertical
                    },
                    percent: None,
                    focus: !detach,
                };
                request(tx, Request::Split(opts)).await.map(drop)
            }
            Command::NewWindow { name, argv, cwd } => {
                let opts = NewWindowOpts {
                    spawn: spawn_opts(name.clone(), argv, cwd),
                    name,
                    focus: true,
                };
                request(tx, Request::NewWindow(opts)).await.map(drop)
            }
            Command::KillPane { target } => {
                let target = match target {
                    Some(target) => target,
                    None => pane(ctx)?,
                };
                ask_actor(tx, SESSION_GONE, |reply| SessionMsg::Kill {
                    target,
                    origin: Origin::Hook,
                    reply,
                })
                .await?
                .map(drop)
            }
            Command::KillWindow { index } => {
                let index = index.map(|i| window(ctx, i)).transpose()?;
                ask_actor(tx, SESSION_GONE, |reply| SessionMsg::KillWindow { index, reply })
                    .await?
                    .map(drop)
            }
            Command::RenameWindow { name } => request(
                tx,
                Request::RenameWindow { index: None, name },
            )
            .await
            .map(drop),
            Command::SelectWindow { index } => request(
                tx,
                Request::SelectWindow {
                    index: window(ctx, index)?,
                },
            )
            .await
            .map(drop),
            Command::NextWindow => request(tx, Request::NextWindow).await.map(drop),
            Command::PrevWindow => request(tx, Request::PrevWindow).await.map(drop),
            Command::LastWindow => request(tx, Request::LastWindow).await.map(drop),
            Command::SelectPane { target } => request(
                tx,
                Request::Focus {
                    name: target,
                    keep_zoom: false,
                },
            )
            .await
            .map(drop),
            Command::NextPane => request(tx, Request::FocusNext).await.map(drop),
            Command::LastPane => request(
                tx,
                Request::FocusLast { keep_zoom: false },
            )
            .await
            .map(drop),
            Command::BreakPane => request(
                tx,
                Request::BreakPane { name: pane(ctx)? },
            )
            .await
            .map(drop),
            Command::SelectLayout { preset } => request(tx, Request::SelectLayout { preset })
                .await
                .map(drop),
            Command::SpreadLayout => request(tx, Request::SpreadLayout).await.map(drop),
            Command::RotateWindow { down } => {
                request(tx, Request::RotateWindow { down }).await.map(drop)
            }
            _ => Err("command not available in hooks".to_string()),
        }
    })
}

/// A script command a hook runs: `run-shell`, `if-shell`, `wait-for` and the hook and alias
/// settings. The commands that list or read scripts are not available in a hook.
async fn run_script(tx: &Tx, ctx: &Context, script: ScriptCommand) -> Result<(), String> {
    match script {
        ScriptCommand::RunShell {
            background,
            target,
            delay_ms,
            command,
        } => {
            let target = target.or_else(|| ctx.pane.clone());
            let command = expand(tx, ctx, &command, target.as_deref()).await?;
            let job = shell::Job {
                command,
                target,
                background,
                delay: delay_ms.map(std::time::Duration::from_millis),
            };
            shell::run(tx, job).await.map(drop)
        }
        ScriptCommand::IfShell {
            background,
            format,
            target,
            condition,
            then,
            otherwise,
        } => {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            let run = async move {
                let target = target.or_else(|| ctx.pane.clone());
                let passed = if format {
                    truthy(&expand(&tx, &ctx, &condition, target.as_deref()).await?)
                } else {
                    let command = expand(&tx, &ctx, &condition, target.as_deref()).await?;
                    let job = shell::Job {
                        command,
                        target,
                        background: false,
                        delay: None,
                    };
                    matches!(
                        shell::run(&tx, job).await?,
                        Response::Shell { status: Some(0), .. }
                    )
                };
                match (passed, otherwise) {
                    (true, _) => run_command(&tx, &ctx, *then).await,
                    (false, Some(otherwise)) => run_command(&tx, &ctx, *otherwise).await,
                    (false, None) => Ok(()),
                }
            };
            if background {
                tokio::spawn(async move {
                    if let Err(e) = run.await {
                        warn!("hook if-shell: {e}");
                    }
                });
                Ok(())
            } else {
                run.await
            }
        }
        ScriptCommand::WaitFor { op, channel } => {
            ask_actor(tx, SESSION_GONE, |reply| {
                SessionMsg::Hook(HookMsg::Channel { channel, op, reply })
            })
            .await?
        }
        ScriptCommand::SetHook {
            name,
            command,
            append,
        } => request(
            tx,
            Request::SetHook {
                name,
                command,
                append,
            },
        )
        .await
        .map(drop),
        ScriptCommand::RunHook { name } => request(tx, Request::RunHook { name }).await.map(drop),
        ScriptCommand::SetAlias { name, command } => request(
            tx,
            Request::SetAlias { name, command },
        )
        .await
        .map(drop),
        ScriptCommand::ShowHooks | ScriptCommand::SourceFile { .. } => {
            Err("command not available in hooks".to_string())
        }
    }
}

/// The hook's pane, for commands that act on one.
fn pane(ctx: &Context) -> Result<String, String> {
    ctx.pane
        .clone()
        .ok_or_else(|| "no pane for this hook".to_string())
}

/// A window number as the user gives it (from `base-index`) as the daemon's index.
fn window(ctx: &Context, index: usize) -> Result<usize, String> {
    index
        .checked_sub(ctx.base_index)
        .ok_or_else(|| format!("can't find window: {index}"))
}

fn spawn_opts(name: Option<String>, argv: Vec<String>, cwd: Option<String>) -> SpawnOpts {
    let (command, args) = match argv.split_first() {
        Some((command, rest)) => (command.clone(), rest.to_vec()),
        None => (String::new(), Vec::new()),
    };
    SpawnOpts {
        name: name.unwrap_or_default(),
        command,
        args,
        pty: true,
        cwd,
        ..SpawnOpts::default()
    }
}

fn truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

/// Sends a request to the session as a hook's: its effects, but no hooks of its own.
async fn request(tx: &Tx, req: Request) -> Result<Response, String> {
    match session::ask_req(tx, Origin::Hook, req).await? {
        Response::Error(e) => Err(e),
        resp => Ok(resp),
    }
}

/// `fmt` with the formats expanded for `target`, and the hook's own variables.
async fn expand(tx: &Tx, ctx: &Context, fmt: &str, target: Option<&str>) -> Result<String, String> {
    let req = Request::Format {
        fmt: fmt.to_string(),
        target: target.map(str::to_string),
        hook: Some(ctx.event.clone()),
    };
    match request(tx, req).await? {
        Response::Text(text) => Ok(text),
        other => Err(format!("unexpected response to a format: {other:?}")),
    }
}
