use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::tools::{AgentLaunch, Call};
use crate::agents::{self, LaunchOptions, state_label};
use crate::client::Client;
use crate::config::Config;
use crate::ipc;
use crate::layout::Direction;
use crate::protocol::{AgentInfo, AgentState, NewWindowOpts, NodeInfo, SpawnOpts, SplitOpts, WaitCondition};

const INVENTORY_LIMIT: usize = 50;
const POLL_SLICE: Duration = Duration::from_millis(200);
const IDLE_FALLBACK_MS: u64 = 2000;

fn node_json(node: NodeInfo) -> Value {
    serde_json::to_value(node).unwrap_or(Value::Null)
}

fn ok() -> Value {
    json!({ "ok": true })
}

/// Performs one typed call against the daemon and returns its JSON result.
pub async fn execute(client: &mut Client, call: Call) -> Result<Value, String> {
    match call {
        Call::Run(spawn) => Ok(node_json(client.spawn(spawn).await?)),
        Call::List => serde_json::to_value(client.list().await?).map_err(|e| e.to_string()),
        Call::Status(name) => Ok(node_json(client.status(&name).await?)),
        Call::Read {
            name,
            tail,
            plain,
            since,
        } => {
            let (text, next) = client.logs(&name, tail, plain, since).await?;
            Ok(json!({ "text": text, "next": next }))
        }
        Call::Screen(name) => Ok(json!({ "text": client.screen(&name).await? })),
        Call::Send { name, data } => client.send_input(&name, &data).await.map(|_| ok()),
        Call::SendKeys {
            name,
            keys,
            literal,
        } => client.send_keys(&name, keys, literal).await.map(|_| ok()),
        Call::Paste { name, text, submit } => {
            client.paste(&name, &text, submit).await.map(|_| ok())
        }
        Call::Wait {
            name,
            condition,
            timeout_ms,
        } => serde_json::to_value(client.wait(&name, condition, timeout_ms).await?)
            .map_err(|e| e.to_string()),
        Call::Kill(name) => client.kill(&name).await.map(|_| ok()),
        Call::Split {
            target,
            direction,
            percent,
            focus,
            spawn,
        } => split(client, target, direction, percent, focus, spawn).await,
        Call::NewWindow { focus, spawn } => {
            let name = Some(spawn.name.clone());
            let node = client
                .new_window(NewWindowOpts { name, spawn, focus })
                .await?;
            Ok(node_json(node))
        }
        Call::Focus(name) => client.focus(&name).await.map(|_| ok()),
        Call::PipeCreate { name, capacity } => {
            client.create_pipe(&name, capacity).await.map(|_| ok())
        }
        Call::PipeWrite { name, data } => {
            Ok(json!({ "seq": client.write_pipe(&name, &data).await? }))
        }
        Call::PipeRead { name, args } => {
            let (messages, next) = client.read_pipe(&name, args).await?;
            Ok(json!({ "messages": messages, "next": next }))
        }
        Call::PipeList => {
            serde_json::to_value(client.list_pipes().await?).map_err(|e| e.to_string())
        }
        Call::Sessions => {
            serde_json::to_value(ipc::session_infos().await).map_err(|e| e.to_string())
        }
        Call::Agents => Ok(inventory().await),
        Call::AgentLaunch(launch) => agent_launch(client, launch).await,
        Call::AgentPrompt {
            name,
            text,
            wait,
            timeout_secs,
        } => {
            agent_prompt(
                client,
                &name,
                &text,
                wait,
                Duration::from_secs(timeout_secs),
            )
            .await
        }
    }
}

async fn inventory() -> Value {
    let registry = agents::registry(&Config::load_or_default());
    agents::inventory_json(&registry, INVENTORY_LIMIT).await
}

async fn split(
    client: &mut Client,
    target: Option<String>,
    direction: Direction,
    percent: Option<u8>,
    focus: bool,
    spawn: SpawnOpts,
) -> Result<Value, String> {
    let node = client
        .split(SplitOpts {
            target,
            spawn,
            direction,
            percent,
            focus,
        })
        .await?;
    Ok(node_json(node))
}

async fn agent_launch(client: &mut Client, launch: AgentLaunch) -> Result<Value, String> {
    let registry = agents::registry(&Config::load_or_default());
    let harness = agents::find_harness(&registry, &launch.harness)
        .ok_or_else(|| format!("unknown harness '{}'", launch.harness))?;
    let spec = agents::launch(
        harness,
        launch.resume.as_deref(),
        LaunchOptions {
            teams: launch.teams,
        },
    );
    let (command, args) = spec
        .argv
        .split_first()
        .ok_or_else(|| format!("harness '{}' has no command", launch.harness))?;
    let name = launch
        .name
        .clone()
        .unwrap_or_else(|| agents::default_node_name(&spec));
    let spawn = SpawnOpts {
        name: name.clone(),
        command: command.clone(),
        args: args.to_vec(),
        pty: true,
        cwd: launch.cwd.clone(),
        env: spec.env.clone(),
        remain_on_exit: None,
        tmux_compat: spec.tmux_compat,
    };
    let node = if launch.split {
        split(client, None, Direction::Horizontal, None, false, spawn).await?
    } else {
        node_json(
            client
                .new_window(NewWindowOpts {
                    name: Some(name),
                    spawn,
                    focus: false,
                })
                .await?,
        )
    };
    Ok(json!({ "node": node, "session_id": spec.session_id }))
}

/// Pastes and submits a prompt, then waits until the agent settles (idle, needs input or exited).
async fn agent_prompt(
    client: &mut Client,
    name: &str,
    text: &str,
    wait: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let before = client.status(name).await?.agent;
    client.paste(name, text, true).await?;
    if !wait {
        return Ok(json!({ "submitted": true }));
    }
    let (state, satisfied) = match before {
        Some(agent) => wait_for_agent(client, name, agent.since_ms, timeout).await?,
        None => {
            let quiet = WaitCondition::Idle {
                quiet_ms: IDLE_FALLBACK_MS,
            };
            let outcome = client
                .wait(name, quiet, Some(timeout.as_millis() as u64))
                .await?;
            (None, outcome.satisfied)
        }
    };
    let screen = client.screen(name).await?;
    Ok(json!({
        "text": screen,
        "state": state.map(state_label),
        "satisfied": satisfied,
    }))
}

/// Next action while polling an agent after a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollStep {
    Finished(AgentState),
    TimedOut(Option<AgentState>),
    Wait(Duration),
}

/// `baseline_ms` is the agent's `since_ms` before the prompt. Only a later transition into a settled
/// state finishes the prompt, so the idle state the agent started from (or a slow start) never counts.
pub fn poll_step(agent: Option<&AgentInfo>, baseline_ms: u64, left: Duration) -> PollStep {
    let settled = agent.filter(|a| a.since_ms > baseline_ms && a.state != AgentState::Working);
    match settled {
        Some(a) => PollStep::Finished(a.state),
        None if left.is_zero() => PollStep::TimedOut(agent.map(|a| a.state)),
        None => PollStep::Wait(left.min(POLL_SLICE)),
    }
}

/// Polls the agent until it settles after `baseline_ms`; returns the state and whether it settled in time.
async fn wait_for_agent(
    client: &mut Client,
    name: &str,
    baseline_ms: u64,
    timeout: Duration,
) -> Result<(Option<AgentState>, bool), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let agent = client.status(name).await?.agent;
        let left = deadline.saturating_duration_since(Instant::now());
        match poll_step(agent.as_ref(), baseline_ms, left) {
            PollStep::Finished(state) => return Ok((Some(state), true)),
            PollStep::TimedOut(state) => return Ok((state, false)),
            PollStep::Wait(slice) => tokio::time::sleep(slice).await,
        }
    }
}
