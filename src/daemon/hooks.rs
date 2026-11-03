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
    BufferRequest, NewWindowOpts, NodeRequest, PaneRequest, PasteBufferOpts, Request, Response,
    ScriptRequest, SessionRequest, SpawnOpts, SplitOpts, WindowRequest,
};
use crate::tui::command::{
    BufferCommand, Command, EnvCommand, PaneCommand, ScriptCommand, SessionCommand, WindowCommand,
};
use crate::tui::panes::neighbor;
use crate::vt::capture::CaptureSpec;

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
            let delay = delay_ms.map(std::time::Duration::from_millis);
            run_shell(tx, ctx, shell::Job { command, target, background, delay }).await
        }
        ScriptCommand::IfShell {
            background,
            format,
            target,
            condition,
            then,
            otherwise,
        } => {
            let check = shell::Job {
                command: condition,
                target,
                background: false,
                delay: None,
            };
            let branch = Branch {
                background,
                format,
                check,
                then,
                otherwise,
            };
            if_shell(tx, ctx, branch).await
        }
        ScriptCommand::WaitFor { op, channel } => {
            ask_actor(tx, SESSION_GONE, |reply| {
                SessionMsg::Hook(HookMsg::Channel { channel, op, reply })
            })
            .await?
        }
        _ => hook_script(tx, script).await,
    }
}

/// Runs `run-shell`: the command is expanded for the job's target, else the hook's pane.
async fn run_shell(tx: &Tx, ctx: &Context, job: shell::Job) -> Result<(), String> {
    let target = job.target.or_else(|| ctx.pane.clone());
    let command = expand(tx, ctx, &job.command, target.as_deref()).await?;
    shell::run(tx, shell::Job { command, target, ..job })
        .await
        .map(drop)
}

/// The two parts of an `if-shell`: its condition, and the command run for each outcome.
struct Branch {
    background: bool,
    format: bool,
    check: shell::Job,
    then: Box<Command>,
    otherwise: Option<Box<Command>>,
}

/// `if-shell`: runs `then` when the condition passes, else `otherwise`. A background branch runs
/// in a task of its own.
async fn if_shell(tx: &Tx, ctx: &Context, branch: Branch) -> Result<(), String> {
    let Branch {
        background,
        format,
        check,
        then,
        otherwise,
    } = branch;
    let (tx, ctx) = (tx.clone(), ctx.clone());
    let run = async move {
        let passed = condition_passes(&tx, &ctx, format, check).await?;
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

/// Whether an `if-shell` condition passes: its expanded text is truthy with `-F`, else the shell
/// command it expands to exits with status 0.
async fn condition_passes(
    tx: &Tx,
    ctx: &Context,
    format: bool,
    check: shell::Job,
) -> Result<bool, String> {
    let target = check.target.or_else(|| ctx.pane.clone());
    let command = expand(tx, ctx, &check.command, target.as_deref()).await?;
    if format {
        return Ok(truthy(&command));
    }
    let job = shell::Job {
        command,
        target,
        background: false,
        delay: None,
    };
    Ok(matches!(
        shell::run(tx, job).await?,
        Response::Shell {
            status: Some(0),
            ..
        }
    ))
}

/// The hook and alias settings, which a hook sets through the session.
async fn hook_script(tx: &Tx, script: ScriptCommand) -> Result<(), String> {
    match script {
        ScriptCommand::SetHook {
            name,
            command,
            append,
        } => script_req(tx, ScriptRequest::SetHook { name, command, append }).await,
        ScriptCommand::RunHook { name } => script_req(tx, ScriptRequest::RunHook { name }).await,
        ScriptCommand::SetAlias { name, command } => {
            script_req(tx, ScriptRequest::SetAlias { name, command }).await
        }
        _ => Err(unavailable()),
    }
}

/// Window commands. Listing and alert navigation read the session's windows first.
async fn window_command(tx: &Tx, ctx: &Context, command: WindowCommand) -> Result<(), String> {
    match command {
        WindowCommand::NewWindow { name, argv, cwd } => new_window(tx, name, argv, cwd).await,
        WindowCommand::KillWindow { index } => kill_window(tx, ctx, index).await,
        WindowCommand::RenameWindow { name } => {
            window_req(tx, WindowRequest::Rename { index: None, name }).await
        }
        WindowCommand::RespawnWindow {
            target,
            kill,
            cwd,
            command,
        } => {
            let index = target.map(|i| window(ctx, i)).transpose()?;
            respawn(tx, command, |command| {
                Request::Window(WindowRequest::Respawn {
                    index,
                    kill,
                    cwd,
                    command,
                })
            })
            .await
        }
        _ => window_select_command(tx, ctx, command).await,
    }
}

/// Window commands that select a window by number, name, or alert, or step to the next one.
async fn window_select_command(
    tx: &Tx,
    ctx: &Context,
    command: WindowCommand,
) -> Result<(), String> {
    match command {
        WindowCommand::SelectWindow { index } => select_window(tx, window(ctx, index)?).await,
        WindowCommand::SelectWindowNamed { name } => select_window_named(tx, name).await,
        WindowCommand::StepAlert { forward } => step_alert(tx, forward).await,
        WindowCommand::NextWindow => window_req(tx, WindowRequest::Next).await,
        _ => window_order_command(tx, ctx, command).await,
    }
}

/// Window commands that step back, rotate, or move a window.
async fn window_order_command(
    tx: &Tx,
    ctx: &Context,
    command: WindowCommand,
) -> Result<(), String> {
    match command {
        WindowCommand::PrevWindow => window_req(tx, WindowRequest::Prev).await,
        WindowCommand::LastWindow => window_req(tx, WindowRequest::Last).await,
        WindowCommand::RotateWindow { down, keep_zoom } => {
            window_req(tx, WindowRequest::Rotate { down, keep_zoom }).await
        }
        WindowCommand::MoveWindow { to } => move_window(tx, window(ctx, to)?).await,
        _ => window_swap_command(tx, ctx, command).await,
    }
}

/// Window commands that renumber or swap windows. Finding a window needs the terminal.
async fn window_swap_command(
    tx: &Tx,
    ctx: &Context,
    command: WindowCommand,
) -> Result<(), String> {
    match command {
        WindowCommand::RenumberWindows => window_req(tx, WindowRequest::Renumber).await,
        WindowCommand::SwapWindow { src, dst } => swap_window(tx, ctx, src, dst).await,
        _ => Err(unavailable()),
    }
}

/// `new-window`: a new window is spawned and focused.
async fn new_window(
    tx: &Tx,
    name: Option<String>,
    argv: Vec<String>,
    cwd: Option<String>,
) -> Result<(), String> {
    let opts = NewWindowOpts {
        spawn: spawn_opts(name.clone(), argv, cwd),
        name,
        focus: true,
    };
    window_req(tx, WindowRequest::New(opts)).await
}

/// `kill-window`: the window is killed as a hook's kill, which fires its own hooks.
async fn kill_window(tx: &Tx, ctx: &Context, index: Option<usize>) -> Result<(), String> {
    let index = index.map(|i| window(ctx, i)).transpose()?;
    ask_actor(tx, SESSION_GONE, |reply| SessionMsg::KillWindow { index, reply })
        .await?
        .map(drop)
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

/// `select-window -t` by name: the first window with that name.
async fn select_window_named(tx: &Tx, name: String) -> Result<(), String> {
    let index = lookup(
        tx,
        |s| s.windows.iter().find(|t| t.name == name).map(|t| t.index),
        &format!("can't find window: {name}"),
    )
    .await?;
    select_window(tx, index).await
}

/// `next-window -a` / `previous-window -a`: selects the next window with an alert.
async fn step_alert(tx: &Tx, forward: bool) -> Result<(), String> {
    let windows = state(tx).await?.windows;
    match alert_window(&windows, forward) {
        Some(index) => select_window(tx, index).await,
        None => Err(format!(
            "no {} window with an alert",
            if forward { "next" } else { "previous" }
        )),
    }
}

/// `move-window -t`: moves the active window to `to`.
async fn move_window(tx: &Tx, to: usize) -> Result<(), String> {
    let from = lookup(tx, active_index, "no active window").await?;
    window_req(tx, WindowRequest::Move { from, to }).await
}

/// `swap-window`: swaps `src` (the active window when `None`) with `dst`.
async fn swap_window(tx: &Tx, ctx: &Context, src: Option<usize>, dst: usize) -> Result<(), String> {
    let a = match src {
        Some(index) => window(ctx, index)?,
        None => lookup(tx, active_index, "no active window").await?,
    };
    let b = window(ctx, dst)?;
    window_req(tx, WindowRequest::Swap { a, b }).await
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
            pane_req(tx, PaneRequest::Split(opts)).await
        }
        PaneCommand::KillPane { target } => kill_pane(tx, target_pane(ctx, target)?).await,
        PaneCommand::SelectPane { target, keep_zoom } => {
            pane_req(
                tx,
                PaneRequest::Focus {
                    name: target,
                    keep_zoom,
                },
            )
            .await
        }
        _ => pane_mark_command(tx, ctx, command).await,
    }
}

/// Pane commands that mark panes, move them into another window, or toggle their input.
async fn pane_mark_command(tx: &Tx, ctx: &Context, command: PaneCommand) -> Result<(), String> {
    match command {
        PaneCommand::MarkPane { target } => {
            pane_req(
                tx,
                PaneRequest::MarkPane {
                    name: target,
                    clear: false,
                },
            )
            .await
        }
        PaneCommand::ClearMark => {
            pane_req(
                tx,
                PaneRequest::MarkPane {
                    name: None,
                    clear: true,
                },
            )
            .await
        }
        PaneCommand::InputOff { target, off } => {
            pane_req(tx, PaneRequest::InputOff { name: target, off }).await
        }
        PaneCommand::JoinPane {
            src,
            dst,
            side_by_side,
            detach,
        } => join_pane(tx, src, target_pane(ctx, dst), side_by_side, detach).await,
        _ => pane_move_command(tx, ctx, command).await,
    }
}

/// Pane commands that restart panes, move focus, or swap panes by the session's order.
async fn pane_move_command(tx: &Tx, ctx: &Context, command: PaneCommand) -> Result<(), String> {
    match command {
        PaneCommand::RespawnPane {
            target,
            kill,
            cwd,
            command,
        } => {
            let name = target_pane(ctx, target)?;
            respawn(tx, command, |command| {
                Request::Node(NodeRequest::Respawn {
                    name,
                    kill,
                    cwd,
                    command,
                })
            })
            .await
        }
        PaneCommand::NextPane => pane_req(tx, PaneRequest::FocusNext).await,
        PaneCommand::LastPane { keep_zoom } => {
            pane_req(tx, PaneRequest::FocusLast { keep_zoom }).await
        }
        PaneCommand::SwapPane { next, keep_zoom } => swap_pane(tx, ctx, next, keep_zoom).await,
        _ => pane_layout_command(tx, ctx, command).await,
    }
}

/// Pane commands that swap marked panes, break a pane out, or change the layout.
async fn pane_layout_command(tx: &Tx, ctx: &Context, command: PaneCommand) -> Result<(), String> {
    match command {
        PaneCommand::SwapPaneMarked {
            source,
            target,
            keep_zoom,
        } => {
            pane_req(
                tx,
                PaneRequest::SwapPane {
                    a: source,
                    b: target,
                    keep_zoom,
                },
            )
            .await
        }
        PaneCommand::BreakPane => pane_req(tx, PaneRequest::BreakPane { name: pane(ctx)? }).await,
        PaneCommand::SelectLayout { preset } => {
            pane_req(tx, PaneRequest::SelectLayout { preset }).await
        }
        PaneCommand::SpreadLayout => pane_req(tx, PaneRequest::SpreadLayout).await,
        _ => Err(unavailable()),
    }
}

/// `join-pane`: `src` defaults to the marked pane, and `dst` is checked after it.
async fn join_pane(
    tx: &Tx,
    src: Option<String>,
    dst: Result<String, String>,
    side_by_side: bool,
    detach: bool,
) -> Result<(), String> {
    let src = match src {
        Some(src) => src,
        None => lookup(tx, |s| s.marked.clone(), "no marked pane").await?,
    };
    let dst = dst?;
    pane_req(
        tx,
        PaneRequest::Join {
            src,
            dst,
            side_by_side,
            detach,
        },
    )
    .await
}

/// `kill-pane`: the pane is killed as a hook's kill, which fires its own hooks.
async fn kill_pane(tx: &Tx, target: String) -> Result<(), String> {
    ask_actor(tx, SESSION_GONE, |reply| SessionMsg::Kill {
        target,
        origin: Origin::Hook,
        reply,
    })
    .await?
    .map(drop)
}

/// `swap-pane -D`/`-U`: swaps the hook's pane with the next or previous pane, if there is one.
async fn swap_pane(tx: &Tx, ctx: &Context, next: bool, keep_zoom: bool) -> Result<(), String> {
    let active = pane(ctx)?;
    let panes = state(tx).await?.panes;
    let other = panes
        .iter()
        .position(|p| *p == active)
        .and_then(|i| neighbor(&panes, i, next))
        .cloned();
    match other {
        Some(other) => {
            pane_req(
                tx,
                PaneRequest::SwapPane {
                    a: Some(active),
                    b: Some(other),
                    keep_zoom,
                },
            )
            .await
        }
        None => Ok(()),
    }
}

/// Paste-buffer and capture commands. Output that a terminal would show (`show-buffer`,
/// `list-buffers`, `capture-pane -p`) is read, so a bad name still fails, and then not shown.
async fn buffer_command(tx: &Tx, ctx: &Context, command: BufferCommand) -> Result<(), String> {
    match command {
        BufferCommand::ListBuffers { .. } => buffer_req(tx, BufferRequest::List).await,
        BufferCommand::SetBuffer {
            name,
            append,
            rename,
            data,
        } => set_buffer(tx, name, append, rename, data).await,
        BufferCommand::ShowBuffer { name } => buffer_req(tx, BufferRequest::Get { name }).await,
        BufferCommand::DeleteBuffer { name } => {
            buffer_req(tx, BufferRequest::Delete { name }).await
        }
        _ => buffer_file_command(tx, ctx, command).await,
    }
}

/// Paste-buffer and capture commands that read or write a file or a pane.
async fn buffer_file_command(
    tx: &Tx,
    ctx: &Context,
    command: BufferCommand,
) -> Result<(), String> {
    match command {
        BufferCommand::LoadBuffer { name, path } => load_buffer(tx, name, &path).await,
        BufferCommand::SaveBuffer { name, append, path } => {
            save_buffer(tx, name, append, &path).await
        }
        BufferCommand::PasteBuffer { opts } => paste_buffer(tx, ctx, opts).await,
        BufferCommand::CapturePane {
            target,
            print,
            buffer,
            spec,
        } => capture_pane(tx, target_pane(ctx, target)?, print, buffer, spec).await,
        _ => Err(unavailable()),
    }
}

/// `set-buffer`: stores `data` in a buffer (appended with `append`), or renames it with `rename`.
async fn set_buffer(
    tx: &Tx,
    name: Option<String>,
    append: bool,
    rename: Option<String>,
    data: Option<String>,
) -> Result<(), String> {
    match (rename, data) {
        (Some(to), None) => buffer_req(tx, BufferRequest::Rename { name, to }).await,
        (Some(_), Some(_)) => Err("set-buffer: -n takes no data".to_string()),
        (None, Some(data)) => buffer_req(tx, BufferRequest::Set { name, data, append }).await,
        (None, None) => Err("set-buffer: no data given".to_string()),
    }
}

/// `load-buffer`: a file becomes a paste buffer.
async fn load_buffer(tx: &Tx, name: Option<String>, path: &str) -> Result<(), String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("load-buffer: {path}: {e}"))?;
    buffer_req(
        tx,
        BufferRequest::Set {
            name,
            data,
            append: false,
        },
    )
    .await
}

/// `paste-buffer`: the pane defaults to the hook's pane.
async fn paste_buffer(tx: &Tx, ctx: &Context, mut opts: PasteBufferOpts) -> Result<(), String> {
    if opts.pane.is_empty() {
        opts.pane = pane(ctx)?;
    }
    buffer_req(tx, BufferRequest::Paste(opts)).await
}

/// `capture-pane`: the text is shown (`print`, not kept here) or stored in buffer `buffer`.
async fn capture_pane(
    tx: &Tx,
    name: String,
    print: bool,
    buffer: Option<String>,
    spec: CaptureSpec,
) -> Result<(), String> {
    let text = match request(tx, Request::Node(NodeRequest::Capture { name, spec })).await? {
        Response::Capture(text) => text,
        other => return Err(format!("unexpected response to capture-pane: {other:?}")),
    };
    if print {
        return Ok(());
    }
    buffer_req(
        tx,
        BufferRequest::Set {
            name: buffer,
            data: text,
            append: false,
        },
    )
    .await
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
    window_req(tx, WindowRequest::Select { index }).await
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

/// The daemon index of the active window.
fn active_index(session: &State) -> Option<usize> {
    session
        .windows
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.index)
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

/// Sends a window request as a hook's, and discards the response.
async fn window_req(tx: &Tx, req: WindowRequest) -> Result<(), String> {
    request(tx, Request::Window(req)).await.map(drop)
}

/// Sends a pane request as a hook's, and discards the response.
async fn pane_req(tx: &Tx, req: PaneRequest) -> Result<(), String> {
    request(tx, Request::Pane(req)).await.map(drop)
}

/// Sends a buffer request as a hook's, and discards the response.
async fn buffer_req(tx: &Tx, req: BufferRequest) -> Result<(), String> {
    request(tx, Request::Buffer(req)).await.map(drop)
}

/// Sends a script request as a hook's, and discards the response.
async fn script_req(tx: &Tx, req: ScriptRequest) -> Result<(), String> {
    request(tx, Request::Script(req)).await.map(drop)
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
