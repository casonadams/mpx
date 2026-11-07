//! Hooks: the commands the daemon runs when an event happens (`set-hook`). A hook keeps the text
//! it was set with and parses it when it runs, with the config's command aliases.
//!
//! Hook commands run in the daemon through the same command language as the TUI's `:` prompt and
//! bindings. The daemon carries out every command that needs no terminal: window, pane, buffer,
//! environment and session commands, `send-keys`, `paste-buffer`, `capture-pane`, zoom, the
//! synchronize setting, and the script commands. Commands that need the terminal (copy mode,
//! prompts, overlays, popups, menus, client commands, key bindings, and selection or resizing by the
//! terminal's geometry) fail with an error. Commands a hook runs do not fire hooks themselves, so a
//! hook cannot loop.
mod buffer;
mod environment;
mod pane;
mod script;
mod window;

use std::collections::BTreeMap;
use std::pin::Pin;

use tokio::sync::mpsc;
use tracing::warn;

use super::SESSION_GONE;
use super::session::{self, HookMsg, Origin, SessionMsg};
use crate::layout::WindowTab;
use crate::node::pane::ask_actor;
use crate::protocol::{
    BufferRequest, NodeRequest, PaneRequest, Request, Response, ScriptRequest, SessionRequest,
    SpawnOpts, WindowRequest,
};
use crate::tui::command::Command;

pub(super) type Tx = mpsc::UnboundedSender<SessionMsg>;

/// The commands of each hook name, in the order they were added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HookTable {
    commands: BTreeMap<String, Vec<String>>,
}

impl HookTable {
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

/// The session facts a command's defaults come from, read when the command runs.
#[derive(Debug, Clone)]
pub(super) struct State {
    /// Every window in order, with the active one flagged.
    pub windows: Vec<WindowTab>,
    /// The marked pane, if any.
    pub marked: Option<String>,
    /// The panes of the active window, in layout order.
    pub panes: Vec<String>,
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
            Command::Script(script) => script::command(tx, ctx, script).await,
            Command::Window(command) => window::command(tx, ctx, command).await,
            Command::Pane(command) => pane::command(tx, ctx, command).await,
            Command::Buffer(command) => buffer::command(tx, ctx, command).await,
            Command::Environment(command) => environment::environment(tx, command).await,
            Command::Session(command) => environment::session(tx, command).await,
            Command::SendKeys { target, keys } => {
                let name = target_pane(ctx, target)?;
                let req = NodeRequest::SendKeys {
                    name,
                    keys,
                    literal: false,
                };
                request(tx, Request::Node(req)).await.map(drop)
            }
            Command::Paste {
                target,
                text,
                submit,
            } => pane::paste(tx, ctx, target, text, submit).await,
            Command::Zoom => {
                pane_req(
                    tx,
                    PaneRequest::Zoom {
                        pane: None,
                        on: None,
                    },
                )
                .await
            }
            Command::SetSynchronize { all, on } => pane::set_synchronize(tx, all, on).await,
            Command::KillServer => {
                ask_actor(tx, SESSION_GONE, |reply| SessionMsg::Shutdown { reply })
                    .await
                    .map(drop)
            }
            _ => Err(unavailable()),
        }
    })
}

/// The error for a command that needs the terminal, which a hook has not got.
fn unavailable() -> String {
    "command not available in hooks".to_string()
}

/// The session's windows and panes, as they are now.
async fn state(tx: &Tx) -> Result<State, String> {
    ask_actor(tx, SESSION_GONE, |reply| {
        SessionMsg::Hook(HookMsg::State { reply })
    })
    .await
}

/// Reads the session state and picks one value from it, or fails with `missing` when there is none.
async fn lookup<T>(
    tx: &Tx,
    select: impl FnOnce(&State) -> Option<T>,
    missing: &str,
) -> Result<T, String> {
    let session = state(tx).await?;
    select(&session).ok_or_else(|| missing.to_string())
}

/// The hook's pane, for commands that act on one.
fn hook_pane(ctx: &Context) -> Result<String, String> {
    ctx.pane
        .clone()
        .ok_or_else(|| "no pane for this hook".to_string())
}

/// The pane a command acts on: its `-t` target, else the hook's pane.
fn target_pane(ctx: &Context, target: Option<String>) -> Result<String, String> {
    target.map_or_else(|| hook_pane(ctx), Ok)
}

/// `new-window` and `split-window`: the spawn options for a command line.
fn spawn_opts(name: Option<String>, argv: Vec<String>, cwd: Option<String>) -> SpawnOpts {
    let mut argv = argv.into_iter();
    SpawnOpts {
        name: name.unwrap_or_default(),
        command: argv.next().unwrap_or_default(),
        args: argv.collect(),
        pty: true,
        cwd,
        ..SpawnOpts::default()
    }
}

/// `respawn-window` and `respawn-pane`: sends the request `build` makes, with `command` when one is
/// given.
async fn respawn(
    tx: &Tx,
    command: Vec<String>,
    build: impl FnOnce(Option<Vec<String>>) -> Request,
) -> Result<(), String> {
    let command = (!command.is_empty()).then_some(command);
    request(tx, build(command)).await.map(drop)
}

/// Sends a request to the session as a hook's: its effects, but no hooks of its own.
async fn request(tx: &Tx, req: Request) -> Result<Response, String> {
    match session::ask_req(tx, Origin::Hook, req).await? {
        Response::Error(e) => Err(e),
        resp => Ok(resp),
    }
}

/// Each sends one request as a hook's, and discards the response.
async fn window_req(tx: &Tx, req: WindowRequest) -> Result<(), String> {
    request(tx, Request::Window(req)).await.map(drop)
}

async fn pane_req(tx: &Tx, req: PaneRequest) -> Result<(), String> {
    request(tx, Request::Pane(req)).await.map(drop)
}

async fn buffer_req(tx: &Tx, req: BufferRequest) -> Result<(), String> {
    request(tx, Request::Buffer(req)).await.map(drop)
}

async fn script_req(tx: &Tx, req: ScriptRequest) -> Result<(), String> {
    request(tx, Request::Script(req)).await.map(drop)
}

async fn session_req(tx: &Tx, req: SessionRequest) -> Result<(), String> {
    request(tx, Request::Session(req)).await.map(drop)
}
