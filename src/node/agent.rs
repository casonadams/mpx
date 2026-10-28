//! Agent tracking for one pane. Pure state transitions; callers pass the clock.

use crate::protocol::{AgentFocus, AgentInfo, AgentState, EDIT_WINDOW_MS, FileEdit, MAX_EDITS};

/// Hook report, mirroring `Request::AgentReport`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub harness: Option<String>,
    pub state: Option<AgentState>,
    pub session_id: Option<String>,
    pub detail: Option<String>,
    pub clear: bool,
    pub task: Option<String>,
    pub reply: Option<String>,
    pub edited: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentTracker {
    harness: Option<String>,
    state: Option<AgentState>,
    session_id: Option<String>,
    detail: Option<String>,
    /// True once a hook reported a state; reported states are authoritative.
    hooked: bool,
    since_ms: u64,
    focus: AgentFocus,
}

/// Records an edit of `path` at `now`: the path moves to the newest end, edits older than
/// `EDIT_WINDOW_MS` drop out and at most `MAX_EDITS` remain.
pub fn record_edit(edits: &mut Vec<FileEdit>, path: &str, now: u64) {
    edits.retain(|e| e.path != path && now.saturating_sub(e.at_ms) < EDIT_WINDOW_MS);
    edits.push(FileEdit {
        path: path.to_string(),
        at_ms: now,
    });
    let excess = edits.len().saturating_sub(MAX_EDITS);
    edits.drain(..excess);
}

/// State of an agent without hooks: `NeedsInput` and `Exited` are sticky, otherwise output
/// recency decides between `Working` and `Idle`.
pub fn unhooked_state(current: Option<AgentState>, quiet_ms: u64, idle_ms: u64) -> AgentState {
    match current {
        Some(s @ (AgentState::NeedsInput | AgentState::Exited)) => s,
        _ if quiet_ms < idle_ms => AgentState::Working,
        _ => AgentState::Idle,
    }
}

impl AgentTracker {
    pub fn is_present(&self) -> bool {
        self.harness.is_some()
    }

    /// True while an unhooked agent is working, so its idle transition needs a timer.
    pub fn ticks(&self) -> bool {
        !self.hooked && self.is_present() && self.state == Some(AgentState::Working)
    }

    pub fn state(&self) -> Option<AgentState> {
        self.state
    }

    pub fn info(&self) -> Option<AgentInfo> {
        Some(AgentInfo {
            harness: self.harness.clone()?,
            state: self.state?,
            session_id: self.session_id.clone(),
            detail: self.detail.clone(),
            since_ms: self.since_ms,
            focus: Box::new(self.focus.clone()),
        })
    }

    /// Foreground process matched a harness (`Some`) or not (`None`). Ignored once hooks report.
    pub fn observe_foreground(&mut self, harness: Option<&str>, now: u64) {
        if self.hooked {
            return;
        }
        match harness {
            Some(h) => {
                if self.harness.as_deref() != Some(h) {
                    self.harness = Some(h.to_string());
                    self.set_state(AgentState::Working, now);
                }
            }
            None => self.clear(),
        }
    }

    pub fn report(&mut self, report: &Report, now: u64) {
        if report.clear {
            self.clear();
            return;
        }
        if let Some(h) = &report.harness {
            self.harness = Some(h.clone());
        } else if self.harness.is_none() && report.state.is_some() {
            self.harness = Some("unknown".to_string());
        }
        if !self.is_present() {
            return;
        }
        if self.state.is_none() {
            self.set_state(AgentState::Working, now);
        }
        if report.session_id.is_some() {
            self.session_id = report.session_id.clone();
        }
        self.note_focus(report, now);
        if let Some(state) = report.state {
            self.hooked = true;
            self.detail = report.detail.clone();
            self.set_state(state, now);
        } else if report.detail.is_some() {
            self.detail = report.detail.clone();
        }
    }

    /// Notification from the terminal (bell or OSC 9/777) while an agent runs.
    pub fn alert(&mut self, detail: Option<String>, now: u64) {
        if !self.is_present() {
            return;
        }
        self.detail = detail;
        self.set_state(AgentState::NeedsInput, now);
    }

    /// User input answers a pending prompt.
    pub fn input(&mut self, now: u64) {
        if self.state == Some(AgentState::NeedsInput) {
            self.detail = None;
            self.set_state(AgentState::Working, now);
        }
    }

    pub fn exited(&mut self, now: u64) {
        if self.is_present() {
            self.set_state(AgentState::Exited, now);
        }
    }

    /// Re-derives the state of an agent without hooks from output recency.
    pub fn tick(&mut self, now: u64, last_output_ms: u64, idle_ms: u64) {
        if self.hooked || !self.is_present() {
            return;
        }
        let quiet = now.saturating_sub(last_output_ms);
        let next = unhooked_state(self.state, quiet, idle_ms);
        self.set_state(next, now);
    }

    fn note_focus(&mut self, report: &Report, now: u64) {
        if report.task.is_some() {
            self.focus.task = report.task.clone();
        }
        if report.reply.is_some() {
            self.focus.reply = report.reply.clone();
        }
        if let Some(path) = &report.edited {
            record_edit(&mut self.focus.edits, path, now);
        }
    }

    fn set_state(&mut self, state: AgentState, now: u64) {
        if self.state != Some(state) {
            self.state = Some(state);
            self.since_ms = now;
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}
