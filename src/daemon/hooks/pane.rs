//! Pane commands: marking, joining, killing, respawning, pasting and the synchronize setting. Those
//! that move focus or swap panes by the terminal's geometry are not here.

use super::{
    Context, Tx, hook_pane, lookup, pane_req, request, respawn, spawn_opts, state, target_pane,
    unavailable,
};
use crate::daemon::SESSION_GONE;
use crate::daemon::session::{Origin, SessionMsg};
use crate::layout::Direction;
use crate::node::pane::ask_actor;
use crate::protocol::{BufferRequest, NodeRequest, PaneRequest, Request, Response, SplitOpts};
use crate::tui::command::PaneCommand;
use crate::tui::panes::neighbor;

pub(super) async fn command(tx: &Tx, ctx: &Context, command: PaneCommand) -> Result<(), String> {
    match command {
        PaneCommand::SplitWindow {
            side_by_side,
            argv,
            cwd,
            detach,
            keep_zoom,
        } => {
            let opts = SplitOpts {
                target: Some(hook_pane(ctx)?),
                spawn: spawn_opts(None, argv, cwd),
                direction: if side_by_side {
                    Direction::Horizontal
                } else {
                    Direction::Vertical
                },
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
        PaneCommand::JoinPane(join) => {
            join_pane(
                tx,
                join.src,
                target_pane(ctx, join.dst),
                join.side_by_side,
                join.detach,
            )
            .await
        }
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
        PaneCommand::BreakPane => {
            pane_req(
                tx,
                PaneRequest::BreakPane {
                    name: hook_pane(ctx)?,
                },
            )
            .await
        }
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
    let active = hook_pane(ctx)?;
    let panes = state(tx).await?.panes;
    let Some(other) = panes
        .iter()
        .position(|p| *p == active)
        .and_then(|i| neighbor(&panes, i, next))
        .cloned()
    else {
        return Ok(());
    };
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

/// `paste`: the text goes to the pane. Without text, the newest paste buffer is pasted, as the
/// terminal pastes its last copy.
pub(super) async fn paste(
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
pub(super) async fn set_synchronize(tx: &Tx, all: bool, on: bool) -> Result<(), String> {
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
        pane_req(tx, req).await?;
    }
    Ok(())
}
