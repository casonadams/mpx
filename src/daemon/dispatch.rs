use std::sync::Arc;

use super::pipes::ReadArgs;
use super::state::{Placement, not_found};
use super::{DaemonState, wait};
use crate::buffer::strip_ansi;
use crate::keys::{encode_key_names, wrap_paste};
use crate::layout::Workspace;
use crate::node::agent::Report;
use crate::node::process::basename;
use crate::node::{Node, now_ms};
use crate::protocol::{NodeInfo, Request, Response, SplitOpts};

fn unit(result: Result<(), String>) -> Response {
    match result {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error(e),
    }
}

fn node_reply(result: Result<NodeInfo, String>) -> Response {
    match result {
        Ok(info) => Response::Node(info),
        Err(e) => Response::Error(e),
    }
}

fn reply<T>(result: Result<T, String>, ok: impl FnOnce(T) -> Response) -> Response {
    match result {
        Ok(value) => ok(value),
        Err(e) => Response::Error(e),
    }
}

/// Runs a workspace operation on the target pane's canonical name.
fn target_op(
    state: &DaemonState,
    target: &str,
    op: impl FnOnce(&mut Workspace, &str) -> Result<(), String>,
) -> Response {
    match state.target_name(target) {
        Ok(name) => unit(op(&mut state.lock().workspace, &name)),
        Err(e) => Response::Error(e),
    }
}

fn ws_op(state: &DaemonState, op: impl FnOnce(&mut Workspace) -> Result<(), String>) -> Response {
    unit(op(&mut state.lock().workspace))
}

fn key_bytes(keys: &[String], literal: bool, node: &dyn Node) -> Vec<u8> {
    if literal {
        keys.concat().into_bytes()
    } else {
        encode_key_names(keys, &node.modes())
    }
}

fn paste_bytes(text: &str, submit: bool, node: &dyn Node) -> Vec<u8> {
    let pty = node.is_pty();
    let mut bytes = if pty {
        wrap_paste(text, &node.modes())
    } else {
        text.as_bytes().to_vec()
    };
    if submit {
        bytes.push(if pty { b'\r' } else { b'\n' });
    }
    bytes
}

/// Log text and the offset to resume from. `plain` strips escapes from PTY output.
fn render_logs(
    node: &dyn Node,
    tail: Option<usize>,
    plain: bool,
    since: Option<u64>,
) -> (String, u64) {
    match since {
        Some(offset) => {
            let (bytes, next) = node.read_since(offset);
            let text = if plain && node.is_pty() {
                strip_ansi(&bytes)
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            (text, next)
        }
        None => {
            let text = if plain {
                node.scrollback_text(tail)
            } else {
                node.read_buffer(tail)
            };
            (text, node.total_written())
        }
    }
}

fn split(state: &Arc<DaemonState>, opts: SplitOpts) -> Response {
    let SplitOpts {
        target,
        spawn,
        direction,
        percent,
        focus,
    } = opts;
    let target = match target.map(|t| state.target_name(&t)).transpose() {
        Ok(target) => target,
        Err(e) => return Response::Error(e),
    };
    node_reply(state.add_node(
        spawn,
        Placement::Split {
            target,
            dir: direction,
            percent,
            focus,
        },
    ))
}

fn layout(state: &DaemonState, width: u16, height: u16) -> Response {
    let mut inner = state.lock();
    state.refresh_window_names(&mut inner);
    let (rects, dividers) = inner.workspace.compute_layout_with_dividers(width, height);
    let tabs = inner.workspace.window_tabs();
    Response::Layout(rects, dividers, tabs)
}

fn swap(state: &DaemonState, a: &str, b: &str) -> Response {
    match (state.target_name(a), state.target_name(b)) {
        (Ok(a), Ok(b)) => ws_op(state, |ws| {
            if ws.swap(&a, &b) {
                Ok(())
            } else {
                Err("Cannot swap panes".to_string())
            }
        }),
        (Err(e), _) | (_, Err(e)) => Response::Error(e),
    }
}

pub(super) async fn dispatch(state: &Arc<DaemonState>, req: Request) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Info => Response::Session(state.session_info()),
        Request::Shutdown => {
            state.stop_nodes().await;
            Response::Ok
        }
        Request::Spawn(opts) => node_reply(state.add_node(opts, Placement::Spawn)),
        Request::List => Response::NodeList(state.list_nodes()),
        Request::Logs {
            name,
            tail,
            plain,
            since,
        } => reply(
            state.with_entry(&name, |e| render_logs(e.node.as_ref(), tail, plain, since)),
            |(data, next)| Response::LogChunk { data, next },
        ),
        Request::Send { name, data } => unit(state.send_with(&name, |_| data.into_bytes())),
        Request::SendKeys {
            name,
            keys,
            literal,
        } => unit(state.send_with(&name, |node| key_bytes(&keys, literal, node))),
        Request::Paste { name, text, submit } => {
            unit(state.send_with(&name, |node| paste_bytes(&text, submit, node)))
        }
        Request::Kill { name } => unit(state.kill_node(&name).await),
        Request::Wait {
            name,
            condition,
            timeout_ms,
        } => reply(
            wait::wait_for(state, &name, condition, timeout_ms).await,
            Response::WaitResult,
        ),
        Request::Subscribe { .. } | Request::SubscribePipe { .. } => {
            Response::Error("subscriptions require a streaming connection".to_string())
        }
        Request::Screen { name } => reply(
            state.with_entry(&name, |e| e.node.read_screen()),
            Response::Screen,
        ),
        Request::Split(opts) => split(state, opts),
        Request::Layout { width, height } => layout(state, width, height),
        Request::FocusNext => ws_op(state, |ws| {
            ws.focus_next();
            Ok(())
        }),
        Request::FocusLast => ws_op(state, |ws| {
            if ws.focus_last() {
                Ok(())
            } else {
                Err("No previous pane".to_string())
            }
        }),
        Request::Focus { name } => target_op(state, &name, |ws, n| {
            if ws.focus(n) {
                Ok(())
            } else {
                Err(not_found(n))
            }
        }),
        Request::ResizePane { name, cols, rows } => unit(
            state
                .with_entry(&name, |e| {
                    e.node.resize(cols, rows).map_err(|e| e.to_string())
                })
                .and_then(|r| r),
        ),
        Request::ResizeSplit {
            name,
            side,
            cells,
            width,
            height,
        } => target_op(state, &name, |ws, n| {
            if ws.resize_split(n, side, cells, width, height) {
                Ok(())
            } else {
                Err("No split border to resize".to_string())
            }
        }),
        Request::SwapPane { a, b } => swap(state, &a, &b),
        Request::BreakPane { name } => target_op(state, &name, |ws, n| {
            if ws.break_pane(n) {
                Ok(())
            } else {
                Err("Pane is alone in its window".to_string())
            }
        }),
        Request::SelectLayout { preset } => ws_op(state, |ws| {
            if ws.select_layout(preset) {
                Ok(())
            } else {
                Err("No window".to_string())
            }
        }),
        Request::NewWindow(opts) => {
            let label = opts.name.unwrap_or_else(|| basename(&opts.spawn.command));
            node_reply(state.add_node(
                opts.spawn,
                Placement::Window {
                    label,
                    focus: opts.focus,
                },
            ))
        }
        Request::NextWindow => ws_op(state, |ws| {
            ws.next_window();
            Ok(())
        }),
        Request::PrevWindow => ws_op(state, |ws| {
            ws.prev_window();
            Ok(())
        }),
        Request::LastWindow => ws_op(state, |ws| {
            if ws.last_window_switch() {
                Ok(())
            } else {
                Err("No last window".to_string())
            }
        }),
        Request::SelectWindow { index } => ws_op(state, |ws| {
            if ws.select_window(index) {
                Ok(())
            } else {
                Err("Window index out of bounds".to_string())
            }
        }),
        Request::RenameWindow { index, name } => ws_op(state, |ws| {
            let idx = index.unwrap_or(ws.active_idx);
            if ws.rename_window(idx, name) {
                Ok(())
            } else {
                Err("Window index out of bounds".to_string())
            }
        }),
        Request::KillWindow { index } => unit(state.kill_window(index).await),
        Request::GetCwd { name } => reply(state.with_entry(&name, |e| e.node.cwd()), Response::Cwd),
        Request::CreatePipe { name, capacity } => {
            state.create_pipe(&name, capacity);
            Response::Ok
        }
        Request::WritePipe { name, data } => Response::PipeSeq(state.append_pipe(&name, &data)),
        Request::ReadPipe {
            name,
            since,
            consumer,
            wait_ms,
            limit,
        } => reply(
            state
                .read_pipe(
                    &name,
                    ReadArgs {
                        since,
                        consumer,
                        wait_ms,
                        limit,
                    },
                )
                .await,
            |(messages, next)| Response::PipeMessages { messages, next },
        ),
        Request::ListPipes => Response::PipeList(state.list_pipes()),
        Request::Status { name } => node_reply(
            state
                .target_name(&name)
                .and_then(|n| state.info_by_name(&n, true)),
        ),
        Request::AgentReport {
            name,
            harness,
            state: reported,
            session_id,
            detail,
            clear,
        } => {
            let report = Report {
                harness,
                state: reported,
                session_id,
                detail,
                clear,
            };
            unit(state.with_entry(&name, |e| e.agent.report(&report, now_ms())))
        }
    }
}
