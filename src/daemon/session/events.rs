//! Hooks and alerts: the commands a session event runs, the bell, activity and silence alerts an
//! output event raises, and the messages that hooks and their scripts send back to the session.

use tracing::warn;

use super::{HookMsg, Reply, Session};
use crate::daemon::hooks::{self, Context, State};
use crate::protocol::{ChannelOp, StreamEvent};
use crate::tui::command::{Command, parse_scoped};

impl Session {
    pub(super) async fn hook(&mut self, msg: HookMsg) {
        match msg {
            HookMsg::Channel { channel, op, reply } => self.channel(&channel, op, reply),
            HookMsg::ShellEnv { target, reply } => {
                let env = self.shell_env(target).await;
                let _ = reply.send(env);
            }
            HookMsg::Event { event, pane } => self.fire(event, pane),
            HookMsg::Parse { line, reply } => {
                let _ = reply.send(self.parse_script(&line));
            }
            HookMsg::State { reply } => {
                let _ = reply.send(State {
                    windows: self.workspace.window_tabs(),
                    marked: self.workspace.marked_pane().map(str::to_string),
                    panes: self
                        .workspace
                        .active_window()
                        .map(|w| w.leaves())
                        .unwrap_or_default(),
                });
            }
        }
    }

    /// `line` as a command with the session's aliases, and the hook context it runs in: the active
    /// pane is the target the commands act on by default.
    fn parse_script(&self, line: &str) -> Result<(Command, Context), String> {
        let command = parse_scoped(line, &self.config.alias, &self.session)?;
        let ctx = Context {
            event: String::new(),
            pane: self.active_pane(),
            base_index: self.config.base_index,
        };
        Ok((command, ctx))
    }

    /// Runs the commands of hook `event` for `pane`, when any are set.
    pub(super) fn fire(&self, event: &str, pane: Option<String>) {
        let lines = self.hooks.commands(event);
        if lines.is_empty() {
            return;
        }
        let commands = lines
            .iter()
            .filter_map(
                |line| match parse_scoped(line, &self.config.alias, &self.session) {
                    Ok(command) => Some(command),
                    Err(e) => {
                        warn!("hook {event}: {line}: {e}");
                        None
                    }
                },
            )
            .collect();
        let ctx = Context {
            event: event.to_string(),
            pane,
            base_index: self.config.base_index,
        };
        hooks::spawn(self.tx.clone(), ctx, commands);
    }

    /// Fires `after-select-window` and `after-select-pane` for what a selection changed.
    pub(super) fn fire_selection(&self, window: usize, pane: Option<String>) {
        if self.workspace.active_idx != window {
            self.fire("after-select-window", self.active_pane());
        }
        if self.active_pane() != pane {
            self.fire("after-select-pane", self.active_pane());
        }
    }

    /// The ids of the session's indices, to tell which were added or removed by a message.
    pub(super) fn link_ids(&self) -> Vec<usize> {
        self.workspace.links.iter().map(|l| l.id).collect()
    }

    /// Fires `window-linked` for each index added since `before` and `window-unlinked` for each one
    /// removed, unless `fire` is off.
    pub(super) fn window_events(&self, before: &[usize], fire: bool) {
        if !fire {
            return;
        }
        for (link, window) in self.workspace.linked() {
            if !before.contains(&link.id) {
                self.fire("window-linked", window.focused.clone());
            }
        }
        for id in before {
            if !self.workspace.links.iter().any(|l| l.id == *id) {
                self.fire("window-unlinked", None);
            }
        }
    }

    /// Fires the bell and activity alerts an output event raises, then relays the event.
    pub(super) fn pane_event(&mut self, event: StreamEvent) {
        if let StreamEvent::Output { name, data, .. } = &event {
            if self.config.window.monitor_bell
                && data.contains('\x07')
                && self.workspace.mark_bell(name)
            {
                self.fire("alert-bell", Some(name.clone()));
            }
            if self.config.window.monitor_activity && self.workspace.mark_activity(name) {
                self.fire("alert-activity", Some(name.clone()));
            }
            self.workspace.note_output(name, std::time::Instant::now());
        }
        self.fanout(event);
    }

    /// When the next background window goes quiet, if `monitor-silence` is on.
    pub(super) fn silence_due(&self) -> Option<tokio::time::Instant> {
        let secs = self.config.window.monitor_silence;
        if secs == 0 {
            return None;
        }
        self.workspace
            .next_silence(secs)
            .map(tokio::time::Instant::from_std)
    }

    /// Flags background windows that have been quiet for `monitor-silence`, and fires
    /// `alert-silence` for each window flagged now.
    pub(super) fn expire_silence(&mut self) {
        let flagged = self.workspace.expire_silence(
            self.config.window.monitor_silence,
            std::time::Instant::now(),
        );
        if flagged.is_empty() {
            return;
        }
        for number in flagged {
            let pane = self.workspace.focused_at(number);
            self.fire("alert-silence", pane);
        }
        self.fanout(StreamEvent::Layout);
    }

    /// `wait-for`: a signal or unlock answers at once; a wait or lock answers when the channel
    /// lets it through.
    fn channel(&mut self, channel: &str, op: ChannelOp, reply: Reply<Result<(), String>>) {
        let result = match op {
            ChannelOp::Wait => return self.channels.wait(channel, reply),
            ChannelOp::Lock => return self.channels.lock_channel(channel, reply),
            ChannelOp::Signal => {
                self.channels.signal(channel);
                Ok(())
            }
            ChannelOp::Unlock => self.channels.unlock(channel),
        };
        let _ = reply.send(result);
    }

    /// The program `run-shell` runs and the directory it starts in: the target pane's, when it
    /// has one.
    async fn shell_env(&self, target: Option<String>) -> Result<(String, Option<String>), String> {
        let program = self
            .config
            .session
            .default_shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "sh".to_string());
        let cwd = match target.or_else(|| self.active_pane()) {
            Some(target) => self.cwd(&target).await?,
            None => None,
        };
        Ok((program, cwd))
    }
}
