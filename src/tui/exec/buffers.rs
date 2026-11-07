//! Paste buffers: set, show, list, rename, delete, paste, load and save, and `capture-pane`.
use crate::format::{buffer_vars, expand};
use crate::protocol::PasteBufferOpts;
use crate::tui::app::{App, Flow};
use crate::tui::command::BufferCommand;
use crate::tui::overlay::{Overlay, buffer_summary};
use crate::tui::session::pick;
use crate::vt::capture::CaptureSpec;
use std::io::Write;

impl App {
    pub(super) async fn buffer_command(&mut self, cmd: BufferCommand) -> Flow {
        match cmd {
            BufferCommand::ListBuffers { format } => self.list_buffers(format).await,
            BufferCommand::SetBuffer {
                name,
                append,
                rename,
                data,
            } => self.set_buffer(name, append, rename, data).await,
            BufferCommand::ShowBuffer { name } => self.show_buffer(name).await,
            BufferCommand::DeleteBuffer { name } => {
                let result = self.client.delete_buffer(name.as_deref()).await;
                self.settle(result)
            }
            BufferCommand::LoadBuffer { name, path } => self.load_buffer(name, &path).await,
            BufferCommand::SaveBuffer { name, append, path } => {
                self.save_buffer(name, append, &path).await
            }
            BufferCommand::PasteBuffer { opts } => self.paste_buffer(opts).await,
            BufferCommand::CapturePane {
                target,
                print,
                buffer,
                spec,
            } => self.capture_pane(target, print, buffer, spec).await,
        }
    }

    /// `list-buffers`: one line per paste buffer, newest first, each rendered by `format`.
    async fn list_buffers(&mut self, format: Option<String>) -> Flow {
        let lines: Result<Vec<String>, String> =
            self.client
                .list_buffers()
                .await
                .map(|buffers| match format {
                    Some(format) => buffers
                        .iter()
                        .map(|buffer| expand(&format, &buffer_vars(buffer)))
                        .collect(),
                    None => buffers.iter().map(buffer_summary).collect(),
                });
        self.show_lines("list-buffers", lines).await
    }

    /// `show-buffer`: the text of a paste buffer, in an overlay.
    async fn show_buffer(&mut self, name: Option<String>) -> Flow {
        let lines: Result<Vec<String>, String> = self
            .client
            .get_buffer(name.as_deref())
            .await
            .map(|buffer| buffer.data.lines().map(str::to_string).collect());
        self.show_lines("show-buffer", lines).await
    }

    /// `set-buffer`: stores `data`, appending with `append`, or renames the buffer to `rename`.
    async fn set_buffer(
        &mut self,
        name: Option<String>,
        append: bool,
        rename: Option<String>,
        data: Option<String>,
    ) -> Flow {
        let result = match (rename, data) {
            (Some(to), None) => self.client.rename_buffer(name.as_deref(), &to).await,
            (Some(_), Some(_)) => Err("set-buffer: -n takes no data".to_string()),
            (None, Some(data)) => self.client.set_buffer(name.as_deref(), &data, append).await,
            (None, None) => Err("set-buffer: no data given".to_string()),
        };
        self.settle(result)
    }

    /// `load-buffer`: a file's contents become a paste buffer.
    async fn load_buffer(&mut self, name: Option<String>, path: &str) -> Flow {
        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(e) => return self.say(format!("load-buffer: {path}: {e}")),
        };
        let result = self.client.set_buffer(name.as_deref(), &data, false).await;
        self.settle(result)
    }

    /// `save-buffer`: a paste buffer is written to a file.
    async fn save_buffer(&mut self, name: Option<String>, append: bool, path: &str) -> Flow {
        if path == "-" {
            return self.say("save-buffer: - (stdout) works only from the command line");
        }
        let buffer = match self.client.get_buffer(name.as_deref()).await {
            Ok(buffer) => buffer,
            Err(e) => return self.say(format!("save-buffer: {e}")),
        };
        let written = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .and_then(|mut file| file.write_all(buffer.data.as_bytes()));
        match written {
            Ok(()) => self.say(format!("saved {} to {path}", buffer.name)),
            Err(e) => self.say(format!("save-buffer: {path}: {e}")),
        }
    }

    /// `paste-buffer`: the buffer goes into the pane, which must be in this session.
    async fn paste_buffer(&mut self, opts: PasteBufferOpts) -> Flow {
        let target = (!opts.pane.is_empty()).then(|| opts.pane.clone());
        let (temp, pane) = match self.target_pane(target.as_deref()).await {
            Ok(found) => found,
            Err(flow) => return flow,
        };
        if temp.is_some() {
            return self.say("paste-buffer: the pane must be in this session");
        }
        let result = self
            .client
            .paste_buffer(PasteBufferOpts { pane, ..opts })
            .await;
        self.settle(result)
    }

    /// `capture-pane`: the pane's text, in an overlay with `-p`, else stored in a paste buffer.
    async fn capture_pane(
        &mut self,
        target: Option<String>,
        print: bool,
        buffer: Option<String>,
        spec: CaptureSpec,
    ) -> Flow {
        let (mut temp, pane) = match self.target_pane(target.as_deref()).await {
            Ok(found) => found,
            Err(flow) => return flow,
        };
        let text = match pick(&mut temp, &mut self.client)
            .capture_pane(&pane, spec)
            .await
        {
            Ok(text) => text,
            Err(e) => return self.say(e),
        };
        if print {
            let lines = text.lines().map(str::to_string).collect();
            return self
                .open_overlay(Overlay::text("capture-pane", lines))
                .await;
        }
        let result = self
            .client
            .set_buffer(buffer.as_deref(), &text, false)
            .await;
        self.settle(result)
    }
}
