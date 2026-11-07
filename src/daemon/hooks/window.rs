//! Window commands. Each sends one window request, or reads the session first when a name or an
//! alert picks the window.

use super::{Context, State, Tx, lookup, respawn, spawn_opts, state, unavailable, window_req};
use crate::daemon::SESSION_GONE;
use crate::daemon::session::SessionMsg;
use crate::layout::WindowTab;
use crate::node::pane::ask_actor;
use crate::protocol::{LinkOpts, NewWindowOpts, Request, ResizeOpts, UnlinkOpts, WindowRequest};
use crate::tui::command::WindowCommand;

/// Window commands. The window numbers the user gives are resolved from `base-index`.
pub(super) async fn command(tx: &Tx, ctx: &Context, command: WindowCommand) -> Result<(), String> {
    match command {
        WindowCommand::NewWindow { name, argv, cwd } => {
            let opts = NewWindowOpts {
                spawn: spawn_opts(name.clone(), argv, cwd),
                name,
                focus: true,
            };
            window_req(tx, WindowRequest::New(opts)).await
        }
        WindowCommand::KillWindow { index } => {
            let index = optional_window(ctx, index)?;
            ask_actor(tx, SESSION_GONE, |reply| SessionMsg::KillWindow {
                index,
                reply,
            })
            .await?
            .map(drop)
        }
        WindowCommand::RenameWindow { name } => {
            window_req(tx, WindowRequest::Rename { index: None, name }).await
        }
        WindowCommand::SelectWindow { index } => select_window(tx, window(ctx, index)?).await,
        WindowCommand::SelectWindowNamed { name } => select_window_named(tx, name).await,
        WindowCommand::StepWindow {
            forward,
            alert: true,
        } => step_alert(tx, forward).await,
        WindowCommand::StepWindow {
            forward,
            alert: false,
        } => {
            let req = if forward {
                WindowRequest::Next
            } else {
                WindowRequest::Prev
            };
            window_req(tx, req).await
        }
        WindowCommand::LastWindow => window_req(tx, WindowRequest::Last).await,
        WindowCommand::RotateWindow { down, keep_zoom } => {
            window_req(tx, WindowRequest::Rotate { down, keep_zoom }).await
        }
        WindowCommand::RespawnWindow {
            target,
            kill,
            cwd,
            command,
        } => {
            let index = optional_window(ctx, target)?;
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
        WindowCommand::MoveWindow { to } => move_window(tx, window(ctx, to)?).await,
        WindowCommand::RenumberWindows => window_req(tx, WindowRequest::Renumber).await,
        WindowCommand::SwapWindow { src, dst } => swap_window(tx, ctx, src, dst).await,
        WindowCommand::LinkWindow(req) => {
            let opts = LinkOpts {
                src: optional_window(ctx, req.src)?,
                dst: optional_window(ctx, req.dst)?,
                place: req.place,
            };
            window_req(tx, WindowRequest::LinkWindow(opts)).await
        }
        WindowCommand::UnlinkWindow(req) => {
            let opts = UnlinkOpts {
                index: optional_window(ctx, req.index)?,
                kill: req.kill,
            };
            window_req(tx, WindowRequest::UnlinkWindow(opts)).await
        }
        WindowCommand::ResizeWindow(req) => {
            let opts = ResizeOpts {
                index: optional_window(ctx, req.index)?,
                resize: req.resize,
            };
            window_req(tx, WindowRequest::ResizeWindow(opts)).await
        }
        WindowCommand::FindWindow { .. } => Err(unavailable()),
    }
}

/// `window` for a number the hook may leave out: `None` stays `None` (the active window).
fn optional_window(ctx: &Context, index: Option<usize>) -> Result<Option<usize>, String> {
    index.map(|i| window(ctx, i)).transpose()
}

/// A window number as the user gives it (from `base-index`) as the daemon's index.
fn window(ctx: &Context, index: usize) -> Result<usize, String> {
    index
        .checked_sub(ctx.base_index)
        .ok_or_else(|| format!("can't find window: {index}"))
}

/// Selects window `index` (a daemon index).
async fn select_window(tx: &Tx, index: usize) -> Result<(), String> {
    window_req(tx, WindowRequest::Select { index }).await
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
