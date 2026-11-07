//! Paste buffer requests.

use super::Session;
use super::answer::unit;
use crate::daemon::stores::no_buffer;
use crate::keys::TermModes;
use crate::node::pane::{InputBuild, TermView};
use crate::protocol::{BufferRequest, PasteBufferOpts, Response};

impl Session {
    pub(super) async fn buffer_request(&mut self, req: BufferRequest) -> Response {
        match req {
            BufferRequest::Set { name, data, append } => {
                self.stores.set_buffer(name, data, append);
                Response::Ok
            }
            BufferRequest::Get { name } => match self.stores.buffer(name.as_deref()) {
                Some(buffer) => Response::Buffer(buffer.clone()),
                None => Response::Error(no_buffer(name.as_deref())),
            },
            BufferRequest::List => Response::Buffers(self.stores.buffers()),
            BufferRequest::Delete { name } => {
                if self.stores.delete_buffer(name.as_deref()) {
                    Response::Ok
                } else {
                    Response::Error(no_buffer(name.as_deref()))
                }
            }
            BufferRequest::Rename { name, to } => {
                unit(self.stores.rename_buffer(name.as_deref(), to))
            }
            BufferRequest::Paste(opts) => self.paste_buffer(opts).await,
        }
    }

    /// `paste-buffer`: pastes the buffer into the pane; `-d` then deletes it.
    async fn paste_buffer(&mut self, opts: PasteBufferOpts) -> Response {
        let Some(buffer) = self.stores.buffer(opts.name.as_deref()).cloned() else {
            return Response::Error(no_buffer(opts.name.as_deref()));
        };
        let PasteBufferOpts {
            pane,
            delete,
            bracketed,
            raw,
            separator,
            ..
        } = opts;
        let data = buffer.data;
        let build: InputBuild = Box::new(move |view: &TermView| {
            let modes = if view.pty {
                view.modes
            } else {
                TermModes::default()
            };
            crate::keys::paste_buffer_bytes(
                &data,
                separator.as_deref(),
                raw || !view.pty,
                bracketed && view.pty,
                &modes,
            )
        });
        match self.send(&pane, build).await {
            Ok(()) => {
                if delete {
                    self.stores.delete_buffer(Some(&buffer.name));
                }
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }
}
