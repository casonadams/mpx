//! Commands that wait on the daemon (`run-shell`, `if-shell`, `wait-for`) run as jobs, so the event
//! loop keeps drawing while they wait. A job's result returns to the loop as a `Job`: the output
//! for the status row, and the commands that followed the waiting command in its line.
use std::time::Duration;

use super::command::Command;
use crate::client::{Client, ShellOutput};
use crate::protocol::{ChannelOp, Request};

/// What a job waits for.
pub enum Work {
    /// `run-shell`, with its formats already expanded.
    Shell {
        command: String,
        target: Option<String>,
        background: bool,
        delay: Option<Duration>,
    },
    /// `if-shell`: runs `condition`, then `then` when it exits 0, else `otherwise`.
    If {
        condition: String,
        target: Option<String>,
        then: Box<Command>,
        otherwise: Option<Box<Command>>,
    },
    /// `wait-for` on a channel.
    Wait { op: ChannelOp, channel: String },
}

/// A finished job: the lines for the status row, or the error, and the commands to run next.
pub struct Job {
    pub outcome: Result<Vec<String>, String>,
    pub then: Vec<Command>,
}

/// Runs `work` on the daemon at `endpoint`; `rest` are the commands after the waiting one.
pub async fn run(endpoint: &str, work: Work, rest: Vec<Command>) -> Job {
    let mut client = match Client::connect(endpoint).await {
        Ok(client) => client,
        Err(e) => {
            return Job {
                outcome: Err(e),
                then: rest,
            };
        }
    };
    match work {
        Work::Shell {
            command,
            target,
            background,
            delay,
        } => {
            let delay_ms = delay.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            let outcome = client
                .run_shell(&command, target.as_deref(), background, delay_ms)
                .await
                .map(|out| out.map_or_else(Vec::new, |out| status_lines(&out)));
            Job {
                outcome,
                then: rest,
            }
        }
        Work::If {
            condition,
            target,
            then,
            otherwise,
        } => {
            let ran = client
                .run_shell(&condition, target.as_deref(), false, None)
                .await;
            let passed = matches!(&ran, Ok(Some(ShellOutput { status: Some(0), .. })));
            let outcome = ran.map(|_| Vec::new());
            let branch = if passed {
                Some(*then)
            } else {
                otherwise.map(|o| *o)
            };
            Job {
                outcome,
                then: branch.into_iter().chain(rest).collect(),
            }
        }
        Work::Wait { op, channel } => {
            let req = Request::WaitFor { channel, op };
            let outcome = client.call_ok("WaitFor", req).await.map(|()| Vec::new());
            Job {
                outcome,
                then: rest,
            }
        }
    }
}

/// The non-empty lines a shell printed, for the status row.
fn status_lines(out: &ShellOutput) -> Vec<String> {
    out.output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}
