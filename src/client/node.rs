use super::Client;
use crate::protocol::{
    NodeInfo, NodeRequest, Request, Response, SpawnOpts, WaitCondition, WaitOutcome,
};
use crate::vt::capture::CaptureSpec;

impl Client {
    pub async fn spawn(&mut self, opts: SpawnOpts) -> Result<NodeInfo, String> {
        request!(self, "Spawn", Request::Node(NodeRequest::Spawn(opts)), Response::Node(info) => *info)
    }

    pub async fn list(&mut self) -> Result<Vec<NodeInfo>, String> {
        request!(self, "List", Request::Node(NodeRequest::List), Response::NodeList(list) => list)
    }

    /// Returns `(data, next_offset)`.
    pub async fn logs(
        &mut self,
        name: &str,
        tail: Option<usize>,
        plain: bool,
        since: Option<u64>,
    ) -> Result<(String, u64), String> {
        let req = Request::Node(NodeRequest::Logs {
            name: name.to_string(),
            tail,
            plain,
            since,
            until: None,
        });
        request!(self, "Logs", req, Response::LogChunk { data, next } => (data, next))
    }

    /// The retained output of `name` before absolute offset `until`, as raw text.
    pub async fn logs_before(&mut self, name: &str, until: u64) -> Result<String, String> {
        let req = Request::Node(NodeRequest::Logs {
            name: name.to_string(),
            tail: None,
            plain: false,
            since: None,
            until: Some(until),
        });
        request!(self, "Logs", req, Response::LogChunk { data, .. } => data)
    }

    pub async fn send_input(&mut self, name: &str, data: &str) -> Result<(), String> {
        let req = Request::Node(NodeRequest::Send {
            name: name.to_string(),
            data: data.to_string(),
        });
        self.call_ok("Send", req).await
    }

    pub async fn send_keys(
        &mut self,
        name: &str,
        keys: Vec<String>,
        literal: bool,
    ) -> Result<(), String> {
        let req = Request::Node(NodeRequest::SendKeys {
            name: name.to_string(),
            keys,
            literal,
        });
        self.call_ok("SendKeys", req).await
    }

    pub async fn paste(&mut self, name: &str, text: &str, submit: bool) -> Result<(), String> {
        let req = Request::Node(NodeRequest::Paste {
            name: name.to_string(),
            text: text.to_string(),
            submit,
        });
        self.call_ok("Paste", req).await
    }

    pub async fn kill(&mut self, name: &str) -> Result<(), String> {
        let req = Request::Node(NodeRequest::Kill {
            name: name.to_string(),
        });
        self.call_ok("Kill", req).await
    }

    pub async fn wait(
        &mut self,
        name: &str,
        condition: WaitCondition,
        timeout_ms: Option<u64>,
    ) -> Result<WaitOutcome, String> {
        let req = Request::Node(NodeRequest::Wait {
            name: name.to_string(),
            condition,
            timeout_ms,
        });
        request!(self, "Wait", req, Response::WaitResult(outcome) => outcome)
    }

    pub async fn screen(&mut self, name: &str) -> Result<String, String> {
        let req = Request::Node(NodeRequest::Screen {
            name: name.to_string(),
        });
        request!(self, "Screen", req, Response::Screen(text) => text)
    }

    /// A pane's live screen as the sequences that rebuild it, the stream offset they are current
    /// to, and the screen's size in columns and rows.
    pub async fn keyframe(&mut self, name: &str) -> Result<(String, u64, (u16, u16)), String> {
        let req = Request::Node(NodeRequest::Keyframe {
            name: name.to_string(),
        });
        self.call_as("Keyframe", req, |resp| match resp {
            Response::Keyframe { data, next, size } => Some((data, next, size)),
            _ => None,
        })
        .await
    }

    /// Text of a pane's history and screen, as `capture-pane` reads it.
    pub async fn capture_pane(&mut self, name: &str, spec: CaptureSpec) -> Result<String, String> {
        let req = Request::Node(NodeRequest::Capture {
            name: name.to_string(),
            spec,
        });
        self.call_as("CapturePane", req, |resp| match resp {
            Response::Capture(text) => Some(text),
            _ => None,
        })
        .await
    }

    pub async fn get_cwd(&mut self, name: &str) -> Result<Option<String>, String> {
        let req = Request::Node(NodeRequest::Cwd {
            name: name.to_string(),
        });
        request!(self, "GetCwd", req, Response::Cwd(cwd) => cwd)
    }

    pub async fn status(&mut self, name: &str) -> Result<NodeInfo, String> {
        let req = Request::Node(NodeRequest::Status {
            name: name.to_string(),
        });
        request!(self, "Status", req, Response::Node(info) => *info)
    }

    pub async fn resize_pane(&mut self, name: &str, cols: u16, rows: u16) -> Result<(), String> {
        let req = Request::Node(NodeRequest::Resize {
            name: name.to_string(),
            cols,
            rows,
        });
        self.call_ok("ResizePane", req).await
    }

    /// Restarts a pane's process in place (`respawn-pane`); see `Request::Node(NodeRequest::Respawn)`.
    pub async fn respawn(
        &mut self,
        name: &str,
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<(), String> {
        let req = Request::Node(NodeRequest::Respawn {
            name: name.to_string(),
            kill,
            cwd,
            command,
        });
        self.call_ok("Respawn", req).await
    }
}
