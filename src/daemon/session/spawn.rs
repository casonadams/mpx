//! Starting panes: where a new pane goes in the layout, the spawn itself, and respawning a pane
//! in place.

use std::sync::Arc;

use super::Placement;
use super::Session;
use super::answer::not_found;
use crate::daemon::plan::{self, PaneIdentity};
use crate::layout::Direction;
use crate::node::SpawnSpec;
use crate::node::pane::{self, PaneCtx, PaneHandle};
use crate::protocol::{NodeInfo, PaneId, Response, SpawnOpts, StreamEvent};

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

impl Session {
    /// Starts a pane and places it in the layout.
    pub(super) async fn add_node(
        &mut self,
        mut opts: SpawnOpts,
        place: Placement,
    ) -> Result<NodeInfo, String> {
        self.resolve_command(&mut opts);
        let id = self.next_pane_id;
        let name = plan::spawn_name(&opts.name, &opts.command, id, |n| {
            self.nodes.contains_key(n)
        });
        self.check_name(&name)?;
        if let Placement::Split {
            target: Some(t), ..
        } = &place
            && self.workspace.window_of(t).is_none()
        {
            return Err(not_found(t));
        }

        let anchor = self.placement_anchor(&place);
        let cwd = match opts.cwd.clone() {
            Some(cwd) => Some(cwd),
            None => self.anchor_cwd(anchor).await.or_else(current_dir),
        };
        // Placed first, so the pane starts at the size it takes in the layout; a pane that fails
        // to start leaves the layout as it was.
        let before = self.workspace.clone();
        self.place(&name, place)?;
        let size = self.pane_size(&name);
        let handle = match self.spawn_pane(&name, id, &opts, cwd, Vec::new(), size) {
            Ok(handle) => handle,
            Err(e) => {
                self.workspace = before;
                return Err(e);
            }
        };

        self.next_pane_id += 1;
        self.register(name.clone(), handle, opts);
        self.info_of(&name, true).await
    }

    /// Checks that a new pane may take `name`: the name is valid and no pane has it.
    pub(super) fn check_name(&self, name: &str) -> Result<(), String> {
        plan::validate_name(name)?;
        if self.nodes.contains_key(name) {
            return Err(format!(
                "Node '{name}' already exists: pass -n NAME to name the new pane differently"
            ));
        }
        Ok(())
    }

    fn placement_anchor(&self, place: &Placement) -> Option<String> {
        let (is_spawn, target) = match place {
            Placement::Spawn => (true, None),
            Placement::Split { target, .. } => (false, target.as_deref()),
            Placement::Window { .. } => (false, None),
        };
        let focused = self.active_pane();
        plan::cwd_anchor(is_spawn, target, focused.as_deref()).map(str::to_string)
    }

    /// Live working directory of a pane, which a new pane inherits.
    async fn anchor_cwd(&self, anchor: Option<String>) -> Option<String> {
        let handle = self.nodes.get(&anchor?)?.handle.clone();
        handle.cwd().await.ok().flatten()
    }

    fn place(&mut self, name: &str, place: Placement) -> Result<(), String> {
        match place {
            Placement::Spawn => {
                self.workspace.add_or_split_with_focus(
                    name.to_string(),
                    Direction::Horizontal,
                    true,
                );
                Ok(())
            }
            Placement::Split {
                target,
                dir,
                percent,
                focus,
            } => {
                self.workspace
                    .split_pane(target.as_deref(), name.to_string(), dir, percent, focus)
            }
            Placement::Window {
                label,
                focus,
                named,
            } => {
                let number =
                    self.workspace
                        .new_window_with_focus(label.clone(), name.to_string(), focus);
                if named {
                    self.workspace.rename_window(number, label);
                }
                Ok(())
            }
        }
    }

    /// Fills in the program of a pane started without a command: the session's `default-command`
    /// through the default shell, or the default shell itself.
    pub(super) fn resolve_command(&self, opts: &mut SpawnOpts) {
        if !opts.command.is_empty() {
            return;
        }
        let env_shell = std::env::var("SHELL").ok();
        let (command, args) = plan::default_argv(
            self.config.session.default_shell.as_deref(),
            env_shell.as_deref(),
            self.config.session.default_command.as_deref(),
        );
        opts.command = command;
        opts.args = args;
    }

    /// The size (cols, rows) pane `name` has in its window's layout, at the size the window is laid
    /// out at; the window's size when the pane is in no window.
    pub(super) fn pane_size(&self, name: &str) -> (u16, u16) {
        self.workspace
            .pane_rect(name, |number| self.window_size_at(number))
            .filter(|rect| rect.width > 0 && rect.height > 0)
            .map_or_else(
                || self.window_size(self.requested),
                |rect| (rect.width, rect.height),
            )
    }

    /// Starts pane `name` at `size`; it shows `restored` before its process writes.
    pub(super) fn spawn_pane(
        &self,
        name: &str,
        id: PaneId,
        opts: &SpawnOpts,
        cwd: Option<String>,
        restored: Vec<u8>,
        size: (u16, u16),
    ) -> Result<PaneHandle, String> {
        let identity = PaneIdentity {
            endpoint: &self.endpoint,
            session: &self.session,
            name,
            id,
        };
        let (mut extra, env_remove) = self.stores.spawn_environment();
        extra.extend(opts.env.iter().cloned());
        let spec = SpawnSpec {
            name: name.to_string(),
            id,
            command: opts.command.clone(),
            args: opts.args.clone(),
            cwd,
            env: plan::pane_env(&identity, &extra),
            env_remove,
            term: self.config.session.default_terminal.clone(),
            scrollback: self.config.session.scrollback_lines,
            restored,
            size,
        };
        let ctx = PaneCtx {
            notify: Arc::clone(&self.notifier),
            relay: Arc::clone(&self.relay),
            pacing: Arc::clone(&self.pacing),
        };
        pane::spawn(spec, opts.pty, ctx).map_err(|e| e.to_string())
    }

    /// Restarts a pane's process in its place (tmux `respawn-pane`): the pane keeps its name and
    /// layout slot. An exited pane keeps its id. A running one must be killed first (`kill`), and
    /// its replacement takes a new id, so the old process's exit cannot end the new one.
    pub(super) fn respawn(
        &mut self,
        target: &str,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Response {
        match self.spawn_replacement(target, kill, cwd, command) {
            Ok(replacement) => {
                self.commit_respawn(replacement);
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }

    /// Starts the replacement of a pane's process without touching the pane: the process it
    /// replaces is killed only by `commit_respawn`.
    fn spawn_replacement(
        &mut self,
        target: &str,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<Replacement, String> {
        let name = self.target_name(target)?;
        let Some(entry) = self.nodes.get(&name) else {
            return Err(format!("can't respawn pane: {name}"));
        };
        let (exited, old_id) = (entry.exited, entry.handle.id());
        if !exited && !kill {
            return Err(format!("pane %{old_id} still active"));
        }
        let mut opts = entry.opts.clone();
        if let Some(argv) = command.filter(|argv| !argv.is_empty()) {
            let mut argv = argv.into_iter();
            opts.command = argv.next().unwrap_or_default();
            opts.args = argv.collect();
        }
        if cwd.is_some() {
            opts.cwd = cwd;
        }
        let id = if exited { old_id } else { self.next_pane_id };
        let spawn_cwd = opts.cwd.clone().or_else(current_dir);
        let size = self.pane_size(&name);
        let handle = self.spawn_pane(&name, id, &opts, spawn_cwd, Vec::new(), size)?;
        if !exited {
            self.next_pane_id += 1;
        }
        Ok(Replacement {
            name,
            handle,
            opts,
            replaces_running: !exited,
        })
    }

    fn commit_respawn(&mut self, replacement: Replacement) {
        let Replacement {
            name,
            handle,
            opts,
            replaces_running,
        } = replacement;
        if replaces_running {
            drop(self.nodes[&name].handle.request_kill());
        }
        self.register(name.clone(), handle, opts);
        self.fanout(StreamEvent::Respawned { name });
    }

    /// Restarts every pane of window `index` (the active window when `None`), as tmux
    /// `respawn-window`. Without `kill`, a window with a running pane is refused before any of its
    /// panes restarts.
    pub(super) fn respawn_window(
        &mut self,
        index: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Response {
        let Some(number) = index.or_else(|| self.workspace.active_number()) else {
            return Response::Error("No such window".to_string());
        };
        let panes = self.workspace.leaves_at(number);
        if panes.is_empty() {
            return Response::Error("No such window".to_string());
        }
        let running = panes
            .iter()
            .filter_map(|name| self.nodes.get(name))
            .any(|entry| !entry.exited);
        if running && !kill {
            return Response::Error("window still active".to_string());
        }
        let mut replacements = Vec::new();
        for name in panes {
            match self.spawn_replacement(&name, kill, cwd.clone(), command.clone()) {
                Ok(replacement) => replacements.push(replacement),
                Err(e) => {
                    for replacement in replacements {
                        drop(replacement.handle.request_kill());
                    }
                    return Response::Error(e);
                }
            }
        }
        for replacement in replacements {
            self.commit_respawn(replacement);
        }
        Response::Ok
    }
}

/// A pane's new process, spawned and waiting for `commit_respawn` to put it in place.
struct Replacement {
    name: String,
    handle: PaneHandle,
    opts: SpawnOpts,
    /// The process being replaced is still running, so it is killed on commit.
    replaces_running: bool,
}
