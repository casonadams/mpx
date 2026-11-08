//! `run-shell`: a command run by the daemon through the session's shell (`default-shell`, else
//! `$SHELL`, else `sh`), in the working directory of the target pane when there is one.
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tracing::warn;

use super::SESSION_GONE;
use super::hooks::Tx;
use super::session::{HookMsg, SessionMsg};
use crate::node::pane::ask_actor;
use crate::protocol::Response;

/// One `run-shell`: the command text (already expanded), the pane it runs for, and how.
pub struct Job {
    pub command: String,
    pub target: Option<String>,
    pub background: bool,
    pub delay: Option<Duration>,
}

/// Runs `job`. A foreground job answers `Shell` with its exit status and output (stdout, then
/// stderr); a background job answers `Ok` once it is started and its output is discarded.
pub(super) async fn run(tx: &Tx, job: Job) -> Result<Response, String> {
    let (program, cwd) = ask_actor(tx, SESSION_GONE, |reply| {
        SessionMsg::Hook(HookMsg::ShellEnv {
            target: job.target,
            reply,
        })
    })
    .await??;
    let mut process = Command::new(program);
    process.arg("-c").arg(&job.command).stdin(Stdio::null());
    if let Some(dir) = cwd {
        process.current_dir(dir);
    }
    let delay = job.delay;
    let run = async move {
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        process.output().await
    };
    if job.background {
        tokio::spawn(async move {
            if let Err(e) = run.await {
                warn!("run-shell: {e}");
            }
        });
        return Ok(Response::Ok);
    }
    let output = run.await.map_err(|e| format!("run-shell: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(Response::Shell {
        status: output.status.code(),
        output: text,
    })
}
