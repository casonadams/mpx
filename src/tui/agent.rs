//! Agent awareness: the waiting-agent badge, off-screen notifications, jump-to-attention and launch.
use std::collections::BTreeSet;
use std::io::{Write, stdout};

use super::app::{App, Flow};
use super::status::osc9_needs_input;
use crate::agents::{self, LaunchOptions, LiveAgent};
use crate::layout::Direction;
use crate::protocol::{AgentState, NewWindowOpts, NodeInfo, SpawnOpts, SplitOpts};

/// Session-qualified agent identity: (session, node name).
pub type AgentKey = (String, String);

#[derive(Default)]
pub struct AgentWatch {
    /// Agents waiting for input across all sessions, as of the last refresh.
    pub badge: usize,
    pub(super) seen: BTreeSet<AgentKey>,
    /// Waiting agents in other sessions, refreshed every other tick.
    pub(super) others: Vec<AgentKey>,
    pub(super) ticks: u64,
}

pub fn needs_input(node: &NodeInfo) -> bool {
    node.agent
        .as_ref()
        .is_some_and(|a| a.state == AgentState::NeedsInput)
}

/// Waiting agents from live listings that live in a session other than `session`.
pub fn waiting_elsewhere(live: Vec<LiveAgent>, session: &str) -> Vec<AgentKey> {
    live.into_iter()
        .filter(|agent| agent.session != session && needs_input(&agent.node))
        .map(|agent| (agent.session, agent.node.name))
        .collect()
}

/// Every agent waiting for input: this session's nodes plus the cached waiting agents elsewhere.
pub fn waiting_keys(
    session: &str,
    nodes: &[NodeInfo],
    elsewhere: &[AgentKey],
) -> BTreeSet<AgentKey> {
    let mut waiting: BTreeSet<AgentKey> = nodes
        .iter()
        .filter(|node| needs_input(node))
        .map(|node| (session.to_string(), node.name.clone()))
        .collect();
    waiting.extend(elsewhere.iter().cloned());
    waiting
}

/// Agents that started waiting since `seen` and deserve an off-screen notification.
/// Nothing is reported when notifications are disabled; agents with a pane visible in the
/// current window are skipped.
pub fn newly_waiting(
    seen: &BTreeSet<AgentKey>,
    now: &BTreeSet<AgentKey>,
    session: &str,
    visible: &[String],
    notify: bool,
) -> Vec<AgentKey> {
    if !notify {
        return Vec::new();
    }
    now.difference(seen)
        .filter(|(agent_session, name)| {
            !(agent_session.as_str() == session && visible.contains(name))
        })
        .cloned()
        .collect()
}

impl App {
    /// Polls other sessions' waiting agents every other tick and refreshes the badge.
    pub async fn refresh_agents(&mut self) {
        self.watch.ticks += 1;
        if self.watch.ticks % 2 == 1 {
            self.watch.others = waiting_elsewhere(agents::list_live().await, &self.session);
        }
        self.update_waiting();
    }

    /// Recomputes the waiting-agent badge from this session's nodes and the cached waiting agents
    /// elsewhere, and notifies once per newly waiting agent that has no visible pane.
    pub fn update_waiting(&mut self) {
        let waiting = waiting_keys(&self.session, &self.nodes, &self.watch.others);
        let visible: Vec<String> = self.panes.iter().map(|(name, ..)| name.clone()).collect();
        for key in newly_waiting(
            &self.watch.seen,
            &waiting,
            &self.session,
            &visible,
            self.config.agent.notify,
        ) {
            notify(&key.1);
        }
        self.watch.badge = waiting.len();
        self.watch.seen = waiting;
    }

    /// Focuses the oldest agent waiting for input in any session.
    pub async fn next_attention(&mut self) -> Flow {
        let oldest = agents::list_live()
            .await
            .into_iter()
            .filter(|live| needs_input(&live.node))
            .min_by_key(|live| live.node.agent.as_ref().map_or(u64::MAX, |a| a.since_ms));
        let Some(hit) = oldest else {
            return self.say("no agent needs input");
        };
        if hit.session != self.session
            && let Err(e) = self.switch_session(&hit.session).await
        {
            return self.say(e);
        }
        self.zoomed = None;
        let result = self.client.focus(&hit.node.name).await;
        self.settle(result)
    }

    /// Starts a harness in a new window (or a side-by-side split) and focuses it.
    pub async fn launch(
        &mut self,
        harness: String,
        resume: Option<String>,
        cwd: Option<String>,
        split: bool,
    ) -> Flow {
        match self
            .launch_agent(&harness, resume.as_deref(), cwd, split)
            .await
        {
            Ok(()) => Flow::Continue,
            Err(e) => self.say(e),
        }
    }

    async fn launch_agent(
        &mut self,
        harness: &str,
        resume: Option<&str>,
        cwd: Option<String>,
        split: bool,
    ) -> Result<(), String> {
        let registry = agents::registry(&self.config);
        let spec_harness = registry
            .iter()
            .find(|h| h.name == harness)
            .ok_or_else(|| format!("unknown harness '{harness}'"))?;
        let spec = agents::launch(spec_harness, resume, LaunchOptions::default());
        let (command, args) = spec
            .argv
            .split_first()
            .ok_or_else(|| format!("harness '{harness}' has no command"))?;
        let name = self.unique_name(&spec.name);
        let cwd = match cwd {
            Some(dir) => Some(dir),
            None => self.active_cwd().await,
        };
        let spawn = SpawnOpts {
            name: name.clone(),
            command: command.clone(),
            args: args.to_vec(),
            pty: true,
            cwd,
            env: spec.env,
            tmux_compat: spec.tmux_compat,
            ..SpawnOpts::default()
        };
        if split {
            let target = (!self.active_pane.is_empty()).then(|| self.active_pane.clone());
            let opts = SplitOpts {
                target,
                spawn,
                direction: Direction::Horizontal,
                percent: None,
                focus: true,
            };
            self.client.split(opts).await.map(|_| ())
        } else {
            let opts = NewWindowOpts {
                name: Some(name),
                spawn,
                focus: true,
            };
            self.client.new_window(opts).await.map(|_| ())
        }
    }

    /// `base`, or `base-N` for the first N that no node in this session uses.
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |candidate: &str| self.nodes.iter().any(|n| n.name == candidate);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base}-{i}"))
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| base.to_string())
    }
}

/// Writes an OSC 9 desktop notification (the sequence carries its own BEL terminator).
fn notify(name: &str) {
    let mut out = stdout();
    let _ = out.write_all(osc9_needs_input(name).as_bytes());
    let _ = out.flush();
}
