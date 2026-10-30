//! tmux hooks (`set-hook`): command lines the session runs when something happens in it.
use crate::protocol::Request;
use crate::tmux_shim::{EnvTarget, run_in};

/// Every hook the daemon can fire.
pub const HOOK_NAMES: &[&str] = &[
    "after-break-pane",
    "after-join-pane",
    "after-kill-window",
    "after-new-window",
    "after-rename-session",
    "after-rename-window",
    "after-resize-pane",
    "after-select-layout",
    "after-select-pane",
    "after-select-window",
    "after-send-keys",
    "after-split-window",
    "after-swap-pane",
    "client-attached",
    "client-detached",
    "pane-exited",
    "session-closed",
];

/// The hook that follows a successful `req`, if there is one.
pub fn after_request(req: &Request) -> Option<&'static str> {
    Some(match req {
        Request::NewWindow(_) => "after-new-window",
        Request::Split(_) => "after-split-window",
        Request::SelectWindow { .. }
        | Request::NextWindow
        | Request::PrevWindow
        | Request::LastWindow => "after-select-window",
        Request::Focus { .. } | Request::FocusNext | Request::FocusLast => "after-select-pane",
        Request::ResizePane { .. } | Request::ResizeSplit { .. } => "after-resize-pane",
        Request::SelectLayout { .. } | Request::SpreadLayout => "after-select-layout",
        Request::RenameWindow { .. } => "after-rename-window",
        Request::RenameSession { .. } => "after-rename-session",
        Request::BreakPane { .. } => "after-break-pane",
        Request::JoinPane { .. } => "after-join-pane",
        Request::SwapPane { .. } => "after-swap-pane",
        Request::SendKeys { .. } => "after-send-keys",
        _ => return None,
    })
}

/// Runs `line` against `target` on its own thread and runtime: a hook never holds up the session
/// that fired it, and the commands it runs talk back to that session.
pub fn fire(target: EnvTarget, line: String) -> std::sync::mpsc::Receiver<()> {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            return;
        };
        if let Err(e) = runtime.block_on(run_in(&target, &line)) {
            tracing::warn!("hook '{line}': {e}");
        }
        let _ = done.send(());
    });
    finished
}
