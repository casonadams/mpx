//! Script commands: hooks, command aliases and `source-file`. The shell and channel commands
//! run as sequences, see `sequence`.
use crate::protocol::{Request, Response, ScriptRequest};
use crate::tui::app::{App, Flow};
use crate::tui::command::{Command, ScriptCommand, parse_scoped};

impl App {
    pub(super) async fn run_script(&mut self, script: ScriptCommand) -> Flow {
        match script {
            ScriptCommand::RunShell { .. }
            | ScriptCommand::IfShell { .. }
            | ScriptCommand::WaitFor { .. } => {
                self.run_sequence(vec![Command::Script(script)]).await
            }
            ScriptCommand::SetHook {
                name,
                command,
                append,
            } => {
                let req = Request::Script(ScriptRequest::SetHook {
                    name,
                    command,
                    append,
                });
                let result = self.client.call_ok("SetHook", req).await;
                self.settle(result)
            }
            ScriptCommand::RunHook { name } => {
                let req = Request::Script(ScriptRequest::RunHook { name });
                let result = self.client.call_ok("RunHook", req).await;
                self.settle(result)
            }
            ScriptCommand::ShowHooks => self.show_hooks().await,
            ScriptCommand::SetAlias { name, command } => self.set_alias(name, command).await,
            ScriptCommand::SourceFile { paths, quiet } => self.source_file(paths, quiet).await,
        }
    }

    async fn show_hooks(&mut self) -> Flow {
        let lines = match self
            .client
            .call(Request::Script(ScriptRequest::ShowHooks))
            .await
        {
            Ok(Response::Hooks(hooks)) => Ok(hooks
                .into_iter()
                .map(|(name, command)| format!("{name} {command}"))
                .collect()),
            Ok(Response::Error(e)) | Err(e) => Err(e),
            Ok(other) => Err(format!("unexpected response to show-hooks: {other:?}")),
        };
        self.show_lines("hooks", lines).await
    }

    /// `set -s command-alias`: the alias is set in the daemon and here, and bindings made from now
    /// on expand it.
    async fn set_alias(&mut self, name: String, command: Option<String>) -> Flow {
        let req = Request::Script(ScriptRequest::SetAlias {
            name: name.clone(),
            command: command.clone(),
        });
        let result = self.client.call_ok("SetAlias", req).await;
        if result.is_ok() {
            match &command {
                Some(command) => self.config.alias.insert(name, command.clone()),
                None => self.config.alias.remove(&name),
            };
            self.apply_bindings().await;
        }
        self.settle(result)
    }

    /// `source-file`: runs the commands of each file, in order. A line that does not parse is
    /// reported and skipped; a missing file is reported unless `-q` silences it.
    async fn source_file(&mut self, paths: Vec<String>, quiet: bool) -> Flow {
        let vars = crate::format::session_vars(
            &self.session,
            self.attachment.times,
            self.attachment.attached,
        );
        for path in paths {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if quiet && e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    self.say(format!("{path}: {e}"));
                    continue;
                }
            };
            let lines = match crate::source::lines(&text, |fmt| crate::format::expand(fmt, &vars)) {
                Ok(lines) => lines,
                Err(e) => {
                    self.say(format!("{path}: {e}"));
                    continue;
                }
            };
            for (line, text) in lines {
                match parse_scoped(&text, &self.config.alias, &self.session) {
                    Ok(command) => {
                        if self.run_sequence(vec![command]).await == Flow::Quit {
                            return Flow::Quit;
                        }
                    }
                    Err(e) => {
                        self.say(format!("{path}:{line}: {e}"));
                    }
                }
            }
        }
        Flow::Continue
    }
}
