//! Script commands a hook runs: `run-shell`, `if-shell`, `wait-for`, and the hook and alias settings.

use std::time::Duration;

use tracing::warn;

use super::{Context, Tx, request, run_command, script_req, unavailable};
use crate::daemon::SESSION_GONE;
use crate::daemon::session::{HookMsg, SessionMsg};
use crate::daemon::shell;
use crate::node::pane::ask_actor;
use crate::protocol::{Request, Response, ScriptRequest};
use crate::tui::command::{Command, ScriptCommand};

/// The script commands. The commands that list or read scripts are not available in a hook.
pub(super) async fn command(tx: &Tx, ctx: &Context, script: ScriptCommand) -> Result<(), String> {
    match script {
        ScriptCommand::RunShell {
            background,
            target,
            delay_ms,
            command,
        } => {
            let job = shell::Job {
                command,
                target,
                background,
                delay: delay_ms.map(Duration::from_millis),
            };
            run_shell(tx, ctx, job).await
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
        ScriptCommand::SetHook {
            name,
            command,
            append,
        } => {
            script_req(
                tx,
                ScriptRequest::SetHook {
                    name,
                    command,
                    append,
                },
            )
            .await
        }
        ScriptCommand::RunHook { name } => script_req(tx, ScriptRequest::RunHook { name }).await,
        ScriptCommand::SetAlias { name, command } => {
            script_req(tx, ScriptRequest::SetAlias { name, command }).await
        }
        _ => Err(unavailable()),
    }
}

/// `run-shell`: the command is expanded for the job's target, else the hook's pane.
async fn run_shell(tx: &Tx, ctx: &Context, job: shell::Job) -> Result<(), String> {
    let job = expand_job(tx, ctx, job).await?;
    shell::run(tx, job).await.map(drop)
}

/// Gives the job the hook's pane when it names no target, and expands its command for that target.
async fn expand_job(tx: &Tx, ctx: &Context, job: shell::Job) -> Result<shell::Job, String> {
    let target = job.target.or_else(|| ctx.pane.clone());
    let command = expand(tx, ctx, &job.command, target.as_deref()).await?;
    Ok(shell::Job {
        command,
        target,
        ..job
    })
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
    let job = expand_job(tx, ctx, check).await?;
    if format {
        return Ok(truthy(&job.command));
    }
    Ok(matches!(
        shell::run(tx, job).await?,
        Response::Shell {
            status: Some(0),
            ..
        }
    ))
}

fn truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
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
