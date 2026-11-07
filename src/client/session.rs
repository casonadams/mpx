use super::{Client, LayoutTuple, unexpected};
use crate::protocol::{
    EnvChange, EnvEntry, Request, Response, ScriptRequest, SessionInfo, SessionRequest,
};

/// What a foreground `run-shell` printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    /// The exit status, `None` when a signal ended the command.
    pub status: Option<i32>,
    pub output: String,
}

/// The answer to a `RunShell`: a foreground run's status and output, or `None` for a background run.
fn shell_reply(resp: Response) -> Result<Option<ShellOutput>, String> {
    match resp {
        Response::Shell { status, output } => Ok(Some(ShellOutput { status, output })),
        Response::Ok => Ok(None),
        other => Err(unexpected("RunShell", other)),
    }
}

impl Client {
    pub async fn ping(&mut self) -> Result<(), String> {
        request!(self, "Ping", Request::Ping, Response::Pong => ())
    }

    pub async fn info(&mut self) -> Result<SessionInfo, String> {
        request!(self, "Info", Request::Info, Response::Session(info) => info)
    }

    pub async fn layout(&mut self, width: u16, height: u16) -> Result<LayoutTuple, String> {
        let req = Request::Session(SessionRequest::Layout { width, height });
        request!(self, "Layout", req, Response::Layout(rects, dividers, tabs) => (rects, dividers, tabs))
    }

    pub async fn restore(&mut self) -> Result<(), String> {
        self.call_ok("Restore", Request::Restore).await
    }

    pub async fn rename_session(&mut self, name: &str) -> Result<(), String> {
        let req = Request::Session(SessionRequest::Rename {
            name: name.to_string(),
        });
        self.call_ok("RenameSession", req).await
    }

    /// Makes the daemon re-read its config from `path` (the defaults when `None`).
    pub async fn reload_config(&mut self, path: Option<&std::path::Path>) -> Result<(), String> {
        let req = Request::Session(SessionRequest::ReloadConfig {
            path: path.map(|p| p.to_string_lossy().into_owned()),
        });
        self.call_ok("ReloadConfig", req).await
    }

    /// Sets (or with `None` forgets) a runtime key binding; see `Request::Session(SessionRequest::SetBinding)`.
    pub async fn set_binding(&mut self, name: &str, line: Option<&str>) -> Result<(), String> {
        let req = Request::Session(SessionRequest::SetBinding {
            name: name.to_string(),
            line: line.map(str::to_string),
        });
        self.call_ok("SetBinding", req).await
    }

    /// The runtime key bindings, sorted by name.
    pub async fn get_bindings(&mut self) -> Result<Vec<(String, String)>, String> {
        request!(self, "GetBindings", Request::Session(SessionRequest::GetBindings), Response::Bindings(bindings) => bindings)
    }

    /// Runs `command` through the daemon's shell (see `Request::Script(ScriptRequest::RunShell)`). A
    /// foreground run answers its exit status and output; a background run answers `None` once it
    /// has started.
    pub async fn run_shell(
        &mut self,
        command: &str,
        target: Option<&str>,
        background: bool,
        delay_ms: Option<u64>,
    ) -> Result<Option<ShellOutput>, String> {
        let req = Request::Script(ScriptRequest::RunShell {
            command: command.to_string(),
            target: target.map(str::to_string),
            background,
            delay_ms,
        });
        shell_reply(self.call(req).await?)
    }

    pub async fn set_environment(&mut self, name: &str, change: EnvChange) -> Result<(), String> {
        let req = Request::Session(SessionRequest::SetEnvironment {
            name: name.to_string(),
            change,
        });
        self.call_ok("SetEnvironment", req).await
    }

    /// The session environment, or only the variable `name`.
    pub async fn show_environment(&mut self, name: Option<&str>) -> Result<Vec<EnvEntry>, String> {
        let req = Request::Session(SessionRequest::ShowEnvironment {
            name: name.map(str::to_string),
        });
        self.call_as("ShowEnvironment", req, |resp| match resp {
            Response::Environment(entries) => Some(entries),
            _ => None,
        })
        .await
    }
}
