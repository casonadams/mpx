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
use std::collections::BTreeMap;
use std::io::Write;
use std::pin::Pin;

use tokio::sync::mpsc;
use tracing::warn;

use super::SESSION_GONE;
use super::session::{self, HookMsg, Origin, SessionMsg};
use super::shell;
use crate::layout::WindowTab;
use crate::node::pane::ask_actor;
use crate::protocol::{
    BufferRequest, NewWindowOpts, NodeRequest, PaneRequest, Request, Response, ScriptRequest,
    SessionRequest, SpawnOpts, SplitOpts, WindowRequest,
};
use crate::tui::command::{
    BufferCommand, Command, EnvCommand, PaneCommand, ScriptCommand, SessionCommand, WindowCommand,
};
use crate::tui::panes::neighbor;

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
            Command::Script(script) => run_script(tx, ctx, script).await,
            Command::Window(command) => window_command(tx, ctx, command).await,
            Command::Pane(command) => pane_command(tx, ctx, command).await,
            Command::Buffer(command) => buffer_command(tx, ctx, command).await,
            Command::Environment(command) => environment_command(tx, command).await,
            Command::Session(command) => session_command(tx, command).await,
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
            } => paste(tx, ctx, target, text, submit).await,
            Command::Zoom => request(
                tx,
                Request::Pane(PaneRequest::Zoom {
                    pane: None,
                    on: None,
                }),
            )
            .await
            .map(drop),
            Command::SetSynchronize { all, on } => set_synchronize(tx, all, on).await,
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
                        Response::Shell {
                            status: Some(0),
                            ..
                        }
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
            Request::Script(ScriptRequest::SetHook {
                name,
                command,
                append,
            }),
        )
        .await
        .map(drop),
        ScriptCommand::RunHook { name } => {
            request(tx, Request::Script(ScriptRequest::RunHook { name }))
                .await
                .map(drop)
        }
        ScriptCommand::SetAlias { name, command } => request(
            tx,
            Request::Script(ScriptRequest::SetAlias { name, command }),
        )
        .await
        .map(drop),
        ScriptCommand::ShowHooks | ScriptCommand::SourceFile { .. } => Err(unavailable()),
    }
}

/// Window commands. Listing and alert navigation read the session's windows first.
async fn window_command(tx: &Tx, ctx: &Context, command: WindowCommand) -> Result<(), String> {
    match command {
        WindowCommand::NewWindow { name, argv, cwd } => {
            let opts = NewWindowOpts {
                spawn: spawn_opts(name.clone(), argv, cwd),
                name,
                focus: true,
            };
            request(tx, Request::Window(WindowRequest::New(opts)))
                .await
                .map(drop)
        }
        WindowCommand::KillWindow { index } => {
            let index = index.map(|i| window(ctx, i)).transpose()?;
            ask_actor(tx, SESSION_GONE, |reply| SessionMsg::KillWindow {
                index,
                reply,
            })
            .await?
            .map(drop)
        }
        WindowCommand::RenameWindow { name } => request(
            tx,
            Request::Window(WindowRequest::Rename { index: None, name }),
        )
        .await
        .map(drop),
        WindowCommand::SelectWindow { index } => select_window(tx, window(ctx, index)?).await,
        WindowCommand::SelectWindowNamed { name } => {
            let index = state(tx)
                .await?
                .windows
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.index)
                .ok_or_else(|| format!("can't find window: {name}"))?;
            select_window(tx, index).await
        }
        WindowCommand::StepAlert { forward } => {
            let windows = state(tx).await?.windows;
            match alert_window(&windows, forward) {
                Some(index) => select_window(tx, index).await,
                None => Err(format!(
                    "no {} window with an alert",
                    if forward { "next" } else { "previous" }
                )),
            }
        }
        WindowCommand::NextWindow => request(tx, Request::Window(WindowRequest::Next))
            .await
            .map(drop),
        WindowCommand::PrevWindow => request(tx, Request::Window(WindowRequest::Prev))
            .await
            .map(drop),
        WindowCommand::LastWindow => request(tx, Request::Window(WindowRequest::Last))
            .await
            .map(drop),
        WindowCommand::RotateWindow { down, keep_zoom } => request(
            tx,
            Request::Window(WindowRequest::Rotate { down, keep_zoom }),
        )
        .await
        .map(drop),
        WindowCommand::RespawnWindow {
            target,
            kill,
            cwd,
            command,
        } => {
            let index = target.map(|i| window(ctx, i)).transpose()?;
            let command = (!command.is_empty()).then_some(command);
            let req = WindowRequest::Respawn {
                index,
                kill,
                cwd,
                command,
            };
            request(tx, Request::Window(req)).await.map(drop)
        }
        WindowCommand::MoveWindow { to } => {
            let to = window(ctx, to)?;
            let from = active_window(tx).await?;
            request(tx, Request::Window(WindowRequest::Move { from, to }))
                .await
                .map(drop)
        }
        WindowCommand::RenumberWindows => request(tx, Request::Window(WindowRequest::Renumber))
            .await
            .map(drop),
        WindowCommand::SwapWindow { src, dst } => {
            let a = match src {
                Some(index) => window(ctx, index)?,
                None => active_window(tx).await?,
            };
            let b = window(ctx, dst)?;
            request(tx, Request::Window(WindowRequest::Swap { a, b }))
                .await
                .map(drop)
        }
        WindowCommand::FindWindow { .. } => Err(unavailable()),
    }
}

/// Pane commands. Those that move focus or swap panes by the terminal's geometry are not here.
async fn pane_command(tx: &Tx, ctx: &Context, command: PaneCommand) -> Result<(), String> {
    match command {
        PaneCommand::SplitWindow {
            side_by_side,
            argv,
            cwd,
            detach,
            keep_zoom,
        } => {
            let opts = SplitOpts {
                target: Some(pane(ctx)?),
                spawn: spawn_opts(None, argv, cwd),
                direction: split_direction(side_by_side),
                percent: None,
                focus: !detach,
                keep_zoom,
            };
            request(tx, Request::Pane(PaneRequest::Split(opts)))
                .await
                .map(drop)
        }
        PaneCommand::KillPane { target } => {
            let target = target_pane(ctx, target)?;
            ask_actor(tx, SESSION_GONE, |reply| SessionMsg::Kill {
                target,
                origin: Origin::Hook,
                reply,
            })
            .await?
            .map(drop)
        }
        PaneCommand::SelectPane { target, keep_zoom } => request(
            tx,
            Request::Pane(PaneRequest::Focus {
                name: target,
                keep_zoom,
            }),
        )
        .await
        .map(drop),
        PaneCommand::MarkPane { target } => request(
            tx,
            Request::Pane(PaneRequest::MarkPane {
                name: target,
                clear: false,
            }),
        )
        .await
        .map(drop),
        PaneCommand::ClearMark => request(
            tx,
            Request::Pane(PaneRequest::MarkPane {
                name: None,
                clear: true,
            }),
        )
        .await
        .map(drop),
        PaneCommand::JoinPane {
            src,
            dst,
            side_by_side,
            detach,
        } => {
            let src = match src {
                Some(src) => src,
                None => state(tx)
                    .await?
                    .marked
                    .ok_or_else(|| "no marked pane".to_string())?,
            };
            let dst = target_pane(ctx, dst)?;
            let req = PaneRequest::Join {
                src,
                dst,
                side_by_side,
                detach,
            };
            request(tx, Request::Pane(req)).await.map(drop)
        }
        PaneCommand::RespawnPane {
            target,
            kill,
            cwd,
            command,
        } => {
            let name = target_pane(ctx, target)?;
            let command = (!command.is_empty()).then_some(command);
            let req = NodeRequest::Respawn {
                name,
                kill,
                cwd,
                command,
            };
            request(tx, Request::Node(req)).await.map(drop)
        }
        PaneCommand::NextPane => request(tx, Request::Pane(PaneRequest::FocusNext))
            .await
            .map(drop),
        PaneCommand::LastPane { keep_zoom } => {
            request(tx, Request::Pane(PaneRequest::FocusLast { keep_zoom }))
                .await
                .map(drop)
        }
        PaneCommand::SwapPane { next, keep_zoom } => {
            let active = pane(ctx)?;
            let panes = state(tx).await?.panes;
            let other = panes
                .iter()
                .position(|p| *p == active)
                .and_then(|i| neighbor(&panes, i, next))
                .cloned();
            match other {
                Some(other) => {
                    let req = PaneRequest::SwapPane {
                        a: Some(active),
                        b: Some(other),
                        keep_zoom,
                    };
                    request(tx, Request::Pane(req)).await.map(drop)
                }
                None => Ok(()),
            }
        }
        PaneCommand::SwapPaneMarked {
            source,
            target,
            keep_zoom,
        } => {
            let req = PaneRequest::SwapPane {
                a: source,
                b: target,
                keep_zoom,
            };
            request(tx, Request::Pane(req)).await.map(drop)
        }
        PaneCommand::BreakPane => request(
            tx,
            Request::Pane(PaneRequest::BreakPane { name: pane(ctx)? }),
        )
        .await
        .map(drop),
        PaneCommand::SelectLayout { preset } => {
            request(tx, Request::Pane(PaneRequest::SelectLayout { preset }))
                .await
                .map(drop)
        }
        PaneCommand::SpreadLayout => request(tx, Request::Pane(PaneRequest::SpreadLayout))
            .await
            .map(drop),
        PaneCommand::SelectPaneDir { .. } | PaneCommand::ResizePane { .. } => Err(unavailable()),
    }
}

/// Paste-buffer and capture commands. Output that a terminal would show (`show-buffer`,
/// `list-buffers`, `capture-pane -p`) is read, so a bad name still fails, and then not shown.
async fn buffer_command(tx: &Tx, ctx: &Context, command: BufferCommand) -> Result<(), String> {
    match command {
        BufferCommand::ListBuffers { .. } => request(tx, Request::Buffer(BufferRequest::List))
            .await
            .map(drop),
        BufferCommand::SetBuffer {
            name,
            append,
            rename,
            data,
        } => match (rename, data) {
            (Some(to), None) => request(tx, Request::Buffer(BufferRequest::Rename { name, to }))
                .await
                .map(drop),
            (Some(_), Some(_)) => Err("set-buffer: -n takes no data".to_string()),
            (None, Some(data)) => request(
                tx,
                Request::Buffer(BufferRequest::Set { name, data, append }),
            )
            .await
            .map(drop),
            (None, None) => Err("set-buffer: no data given".to_string()),
        },
        BufferCommand::ShowBuffer { name } => {
            request(tx, Request::Buffer(BufferRequest::Get { name }))
                .await
                .map(drop)
        }
        BufferCommand::DeleteBuffer { name } => {
            request(tx, Request::Buffer(BufferRequest::Delete { name }))
                .await
                .map(drop)
        }
        BufferCommand::LoadBuffer { name, path } => {
            let data =
                std::fs::read_to_string(&path).map_err(|e| format!("load-buffer: {path}: {e}"))?;
            request(
                tx,
                Request::Buffer(BufferRequest::Set {
                    name,
                    data,
                    append: false,
                }),
            )
            .await
            .map(drop)
        }
        BufferCommand::SaveBuffer { name, append, path } => {
            save_buffer(tx, name, append, &path).await
        }
        BufferCommand::PasteBuffer { mut opts } => {
            if opts.pane.is_empty() {
                opts.pane = pane(ctx)?;
            }
            request(tx, Request::Buffer(BufferRequest::Paste(opts)))
                .await
                .map(drop)
        }
        BufferCommand::CapturePane {
            target,
            print,
            buffer,
            spec,
        } => {
            let name = target_pane(ctx, target)?;
            let text = match request(tx, Request::Node(NodeRequest::Capture { name, spec })).await?
            {
                Response::Capture(text) => text,
                other => return Err(format!("unexpected response to capture-pane: {other:?}")),
            };
            if print {
                return Ok(());
            }
            request(
                tx,
                Request::Buffer(BufferRequest::Set {
                    name: buffer,
                    data: text,
                    append: false,
                }),
            )
            .await
            .map(drop)
        }
    }
}

/// `save-buffer`: a paste buffer is written to a file, appended with `append`.
async fn save_buffer(
    tx: &Tx,
    name: Option<String>,
    append: bool,
    path: &str,
) -> Result<(), String> {
    if path == "-" {
        return Err("save-buffer: - (stdout) works only from the command line".to_string());
    }
    let Response::Buffer(buffer) =
        request(tx, Request::Buffer(BufferRequest::Get { name })).await?
    else {
        return Err("save-buffer: unexpected response".to_string());
    };
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .and_then(|mut file| file.write_all(buffer.data.as_bytes()))
        .map_err(|e| format!("save-buffer: {path}: {e}"))
}

/// Session environment commands.
async fn environment_command(tx: &Tx, command: EnvCommand) -> Result<(), String> {
    let req = match command {
        EnvCommand::SetEnvironment { name, change } => {
            SessionRequest::SetEnvironment { name, change }
        }
        EnvCommand::ShowEnvironment { name } => SessionRequest::ShowEnvironment { name },
    };
    request(tx, Request::Session(req)).await.map(drop)
}

/// Session commands the daemon carries out: renaming. Switching clients between sessions is not
/// available in a hook.
async fn session_command(tx: &Tx, command: SessionCommand) -> Result<(), String> {
    match command {
        SessionCommand::RenameSession { name } => {
            request(tx, Request::Session(SessionRequest::Rename { name }))
                .await
                .map(drop)
        }
        _ => Err(unavailable()),
    }
}

/// `paste`: the text goes to the pane. Without text, the newest paste buffer is pasted, as the
/// terminal pastes its last copy.
async fn paste(
    tx: &Tx,
    ctx: &Context,
    target: Option<String>,
    text: String,
    submit: bool,
) -> Result<(), String> {
    let name = target_pane(ctx, target)?;
    let text = if text.is_empty() {
        match request(tx, Request::Buffer(BufferRequest::Get { name: None })).await {
            Ok(Response::Buffer(buffer)) => buffer.data,
            _ => String::new(),
        }
    } else {
        text
    };
    if text.is_empty() {
        return Ok(());
    }
    request(tx, Request::Node(NodeRequest::Paste { name, text, submit }))
        .await
        .map(drop)
}

/// `set-option synchronize-panes`: the active window, or every window with `all`.
async fn set_synchronize(tx: &Tx, all: bool, on: bool) -> Result<(), String> {
    let windows: Vec<usize> = state(tx)
        .await?
        .windows
        .iter()
        .filter(|t| all || t.is_active)
        .map(|t| t.index)
        .collect();
    for window in windows {
        let req = PaneRequest::Synchronize {
            window: Some(window),
            on: Some(on),
        };
        request(tx, Request::Pane(req)).await?;
    }
    Ok(())
}

/// Selects window `index` (a daemon index).
async fn select_window(tx: &Tx, index: usize) -> Result<(), String> {
    request(tx, Request::Window(WindowRequest::Select { index }))
        .await
        .map(drop)
}

/// The daemon index of the active window.
async fn active_window(tx: &Tx) -> Result<usize, String> {
    state(tx)
        .await?
        .windows
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.index)
        .ok_or_else(|| "no active window".to_string())
}

/// The next (or previous) window with activity, a bell or silence, wrapping around.
fn alert_window(tabs: &[WindowTab], forward: bool) -> Option<usize> {
    let count = tabs.len();
    let current = tabs.iter().position(|t| t.is_active).unwrap_or(0);
    (1..count)
        .map(|step| {
            if forward {
                (current + step) % count
            } else {
                (current + count - step) % count
            }
        })
        .find(|&i| tabs[i].has_activity || tabs[i].has_bell || tabs[i].has_silence)
        .map(|i| tabs[i].index)
}

/// The session's windows and panes, as they are now.
async fn state(tx: &Tx) -> Result<State, String> {
    ask_actor(tx, SESSION_GONE, |reply| {
        SessionMsg::Hook(HookMsg::State { reply })
    })
    .await
}

/// The pane a command acts on: its `-t` target, else the hook's pane.
fn target_pane(ctx: &Context, target: Option<String>) -> Result<String, String> {
    match target {
        Some(target) => Ok(target),
        None => pane(ctx),
    }
}

/// The direction `split-window` splits in: side by side with `-h`, else stacked.
fn split_direction(side_by_side: bool) -> crate::layout::Direction {
    if side_by_side {
        crate::layout::Direction::Horizontal
    } else {
        crate::layout::Direction::Vertical
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
    let req = Request::Script(ScriptRequest::Format {
        fmt: fmt.to_string(),
        target: target.map(str::to_string),
        hook: Some(ctx.event.clone()),
    });
    let resp = request(tx, req).await?;
    let Response::Text(text) = resp else {
        return Err(format!("unexpected response to a format: {resp:?}"));
    };
    Ok(text)
}
