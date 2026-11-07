//! Node requests: spawning, listing, input, screen, sizing, cwd, respawn and status of panes.

use super::Placement;
use super::Session;
use super::answer::{connection_only, node_reply, reply, unit};
use crate::keys::{KeyPolicy, TermModes, encode_key_names, wrap_paste};
use crate::node::pane::{InputBuild, TermView};
use crate::protocol::{NodeRequest, Response};
use crate::vt::capture::CaptureSpec;

impl Session {
    pub(super) async fn node_request(&mut self, req: NodeRequest) -> Response {
        match req {
            NodeRequest::Spawn(opts) => node_reply(self.add_node(opts, Placement::Spawn).await),
            NodeRequest::List => Response::NodeList(self.list().await),
            NodeRequest::Status { name } => node_reply(self.status(&name).await),
            NodeRequest::Send { name, data } => unit(
                self.send(&name, Box::new(move |_: &TermView| data.into_bytes()))
                    .await,
            ),
            NodeRequest::SendKeys {
                name,
                keys,
                literal,
            } => {
                let policy = KeyPolicy::of(&self.config);
                unit(
                    self.send(
                        &name,
                        Box::new(move |view: &TermView| key_bytes(&keys, literal, view, policy)),
                    )
                    .await,
                )
            }
            NodeRequest::Paste { name, text, submit } => unit(
                self.send(
                    &name,
                    Box::new(move |view: &TermView| paste_bytes(&text, submit, view)),
                )
                .await,
            ),
            NodeRequest::Screen { name } => reply(self.screen(&name).await, Response::Screen),
            NodeRequest::Keyframe { name } => {
                reply(self.keyframe(&name).await, |(data, next, size)| {
                    Response::Keyframe { data, next, size }
                })
            }
            NodeRequest::Capture { name, spec } => {
                reply(self.capture(&name, spec).await, Response::Capture)
            }
            NodeRequest::Resize { name, cols, rows } => unit(self.resize(&name, cols, rows).await),
            NodeRequest::Cwd { name } => reply(self.cwd(&name).await, Response::Cwd),
            NodeRequest::Respawn {
                name,
                kill,
                cwd,
                command,
            } => self.respawn(&name, kill, cwd, command),
            NodeRequest::Logs { .. } | NodeRequest::Kill { .. } | NodeRequest::Wait { .. } => {
                connection_only()
            }
        }
    }

    async fn screen(&self, name: &str) -> Result<String, String> {
        self.handle_of(name)?.screen().await
    }

    async fn keyframe(&self, name: &str) -> Result<(String, u64, (u16, u16)), String> {
        self.handle_of(name)?.keyframe().await
    }

    async fn capture(&self, name: &str, spec: CaptureSpec) -> Result<String, String> {
        self.handle_of(name)?.capture(spec).await
    }

    /// Writes input built from the pane's terminal state to pane `name`; nothing is written while
    /// the pane's input is off.
    pub(super) async fn send(&self, name: &str, build: InputBuild) -> Result<(), String> {
        if self.workspace.input_off(name) {
            return Ok(());
        }
        self.handle_of(name)?.input(build).await
    }

    pub(super) async fn cwd(&self, name: &str) -> Result<Option<String>, String> {
        self.handle_of(name)?.cwd().await
    }

    async fn resize(&self, name: &str, cols: u16, rows: u16) -> Result<(), String> {
        self.handle_of(name)?.resize(cols, rows).await
    }
}

/// The bytes `send-keys` writes: the names encoded for the pane's terminal modes, or the keys
/// verbatim with `literal`.
fn key_bytes(keys: &[String], literal: bool, view: &TermView, policy: KeyPolicy) -> Vec<u8> {
    if literal {
        keys.concat().into_bytes()
    } else {
        encode_key_names(keys, &view.modes, policy)
    }
}

/// The bytes `send-paste` writes: the text, bracketed when the pane takes it, then a newline when
/// `submit` is set.
fn paste_bytes(text: &str, submit: bool, view: &TermView) -> Vec<u8> {
    let modes: TermModes = view.modes;
    let mut bytes = if view.pty {
        wrap_paste(text, &modes)
    } else {
        text.as_bytes().to_vec()
    };
    if submit {
        bytes.push(if view.pty { b'\r' } else { b'\n' });
    }
    bytes
}
