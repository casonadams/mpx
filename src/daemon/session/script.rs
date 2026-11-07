//! The script requests the session answers itself: formats, hooks and command aliases.

use super::Session;
use super::answer::{connection_only, reply};
use crate::daemon::clients;
use crate::format::{self, Vars};
use crate::protocol::{HOOK_EVENTS, Response, ScriptRequest};
use crate::tui::command::parse_scoped;

/// The error for an event name that is not a hook, if `name` is not one.
fn unknown_hook(name: &str) -> Option<Response> {
    (!HOOK_EVENTS.contains(&name)).then(|| Response::Error(format!("unknown hook '{name}'")))
}

impl Session {
    /// The script group: formats, hooks and aliases, which the session evaluates itself. The
    /// commands that run a shell or wait on a channel are answered by the connection.
    pub(super) async fn script_request(&mut self, req: ScriptRequest) -> Response {
        match req {
            ScriptRequest::Format { fmt, target, hook } => reply(
                self.format(&fmt, target, hook.as_deref()).await,
                Response::Text,
            ),
            ScriptRequest::SetHook {
                name,
                command,
                append,
            } => self.set_hook(&name, command, append),
            ScriptRequest::RunHook { name } => self.run_hook(&name),
            ScriptRequest::ShowHooks => Response::Hooks(self.hooks.list()),
            ScriptRequest::SetAlias { name, command } => self.set_alias(name, command),
            ScriptRequest::RunShell { .. }
            | ScriptRequest::RunCommand { .. }
            | ScriptRequest::WaitFor { .. } => connection_only(),
        }
    }

    /// `fmt` with the formats expanded for pane `target` (the active pane when `None`).
    async fn format(
        &self,
        fmt: &str,
        target: Option<String>,
        hook: Option<&str>,
    ) -> Result<String, String> {
        let vars = self.format_vars(target, hook).await?;
        Ok(format::expand(fmt, &vars))
    }

    /// The variables of pane `target` (the active pane when `None`), or of the session alone
    /// when there is no pane. A hook adds its own variables: `hook` and `hook_pane`.
    async fn format_vars(
        &self,
        target: Option<String>,
        hook: Option<&str>,
    ) -> Result<Vars, String> {
        let name = match target {
            Some(target) => Some(self.target_name(&target)?),
            None => self.active_pane(),
        };
        let attached = clients::live(&self.filters).len();
        let times = format::SessionTimes {
            started_ms: self.created_ms,
            restored_ms: self.restored_ms,
        };
        let mut vars = match name {
            Some(name) => {
                let node = self.info_of(&name, true).await?;
                let window = node.window.unwrap_or(0);
                let slot = self
                    .workspace
                    .leaves_at(window)
                    .iter()
                    .position(|leaf| leaf == &name)
                    .unwrap_or(0);
                let tabs = self.workspace.window_tabs();
                let mut view = format::pane_view(&self.session, slot, &node, &[], &tabs);
                view.times = times;
                view.window_base = self.config.base_index;
                view.pane_base = self.config.window.pane_base_index;
                view.session_attached = attached;
                format::pane_vars(&view)
            }
            None => format::session_vars(&self.session, times, attached),
        };
        if let Some(event) = hook {
            let pane_id = vars.get("pane_id").cloned().unwrap_or_default();
            vars.insert("hook", event.to_string());
            vars.insert("hook_pane", pane_id);
        }
        Ok(vars)
    }

    /// `set-hook`: the commands of a hook, after checking that the hook exists and they parse.
    fn set_hook(&mut self, name: &str, command: Option<String>, append: bool) -> Response {
        if let Some(error) = unknown_hook(name) {
            return error;
        }
        if let Some(line) = &command
            && let Err(e) = parse_scoped(line, &self.config.alias, &self.session)
        {
            return Response::Error(e);
        }
        self.hooks.set(name, command, append);
        Response::Ok
    }

    /// `set-hook -R`: runs the commands of a hook now, for the active pane.
    fn run_hook(&self, name: &str) -> Response {
        if let Some(error) = unknown_hook(name) {
            return error;
        }
        self.fire(name, self.active_pane());
        Response::Ok
    }

    /// `set -s command-alias`: sets the alias `name`, or removes it.
    fn set_alias(&mut self, name: String, command: Option<String>) -> Response {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Response::Error(format!("invalid alias name '{name}'"));
        }
        match command {
            Some(command) => self.config.alias.insert(name, command),
            None => self.config.alias.remove(&name),
        };
        Response::Ok
    }
}
