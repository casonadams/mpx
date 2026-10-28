//! Gap-free pane output: each screen tracks the byte offset it has absorbed, and every event is
//! checked against that offset. Missing bytes are fetched from the daemon's log.

use super::app::App;
use super::panes::load_pane;
use crate::protocol::{AgentInfo, StreamEvent};
use crate::vt::VtScreen;

/// Size of a screen whose pane has not been sized yet.
const DEFAULT_SIZE: (u16, u16) = (80, 24);

/// What to do with one output chunk, given the offset the pane's screen expects next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPlan {
    /// Continues the stream exactly: feed the chunk.
    Feed,
    /// Already covered by the screen (or the pane has no baseline yet, and its load will cover it).
    Skip,
    /// Bytes are missing before the chunk, or it overlaps the expected offset: fetch from the log.
    Fetch,
}

/// Decides how a chunk spanning `offset..next` applies to a screen expecting `expected`.
pub fn plan_output(expected: Option<u64>, offset: u64, next: u64) -> OutputPlan {
    match expected {
        None => OutputPlan::Skip,
        Some(expected) if next <= expected => OutputPlan::Skip,
        Some(expected) if offset == expected => OutputPlan::Feed,
        Some(_) => OutputPlan::Fetch,
    }
}

impl App {
    /// Applies one event from the current subscription. Returns true when the layout must be refetched.
    pub async fn apply_event(&mut self, event: StreamEvent) -> bool {
        match event {
            StreamEvent::Output {
                name,
                data,
                offset,
                next,
            } => {
                self.apply_output(&name, &data, offset, next).await;
                false
            }
            StreamEvent::Exit { name, .. } => {
                self.vt.remove(&name);
                self.expected.remove(&name);
                self.pane_dims.remove(&name);
                true
            }
            StreamEvent::Agent { name, agent } => {
                self.apply_agent(&name, agent).await;
                false
            }
            StreamEvent::Lagged => {
                self.resync_all().await;
                true
            }
            _ => false,
        }
    }

    async fn apply_output(&mut self, name: &str, data: &str, offset: u64, next: u64) {
        match plan_output(self.expected.get(name).copied(), offset, next) {
            OutputPlan::Skip => {}
            OutputPlan::Feed => self.feed(name, data, next),
            OutputPlan::Fetch => {
                self.catch_up(name).await;
                self.apply_after_fetch(name, data, offset, next).await;
            }
        }
    }

    /// Applies a chunk once the log has been read up to the expected offset; a chunk still out of
    /// step after that means the screen cannot be patched, so the pane is rebuilt.
    async fn apply_after_fetch(&mut self, name: &str, data: &str, offset: u64, next: u64) {
        match plan_output(self.expected.get(name).copied(), offset, next) {
            OutputPlan::Feed => self.feed(name, data, next),
            OutputPlan::Skip => {}
            OutputPlan::Fetch => self.resync_pane(name).await,
        }
    }

    fn feed(&mut self, name: &str, data: &str, next: u64) {
        self.vt
            .entry(name.to_string())
            .or_insert_with(|| VtScreen::new(DEFAULT_SIZE.0 as usize, DEFAULT_SIZE.1 as usize))
            .feed(data.as_bytes());
        self.expected.insert(name.to_string(), next);
    }

    /// Feeds the bytes a pane is missing, read from the log since its expected offset.
    async fn catch_up(&mut self, name: &str) {
        let Some(since) = self.expected.get(name).copied() else {
            return;
        };
        if let Ok((data, next)) = self.client.logs(name, None, false, Some(since)).await {
            if let Some(vt) = self.vt.get_mut(name) {
                vt.feed(data.as_bytes());
            }
            self.expected.insert(name.to_string(), next);
        }
    }

    /// Rebuilds one pane's screen from its full log at its current size.
    async fn resync_pane(&mut self, name: &str) {
        let (width, height) = self.pane_dims.get(name).copied().unwrap_or(DEFAULT_SIZE);
        if let Some((screen, next)) = load_pane(&mut self.client, name, width, height).await {
            self.vt.insert(name.to_string(), screen);
            self.expected.insert(name.to_string(), next);
        }
    }

    /// After the subscriber lagged: every initialized pane is rebuilt from its log.
    async fn resync_all(&mut self) {
        let names: Vec<String> = self.expected.keys().cloned().collect();
        for name in names {
            self.resync_pane(&name).await;
        }
    }

    async fn apply_agent(&mut self, name: &str, agent: Option<AgentInfo>) {
        if !self.nodes.iter().any(|n| n.name == name) {
            self.refresh_nodes().await;
        }
        if let Some(node) = self.nodes.iter_mut().find(|n| n.name == name) {
            node.agent = agent;
        }
        self.update_waiting();
    }
}
