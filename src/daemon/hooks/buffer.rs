//! Paste-buffer and capture commands. Output that a terminal would show (`show-buffer`,
//! `list-buffers`, `capture-pane -p`) is read, so a bad name still fails, and then not shown.

use std::io::Write;

use super::{Context, Tx, buffer_req, hook_pane, request, target_pane};
use crate::protocol::{BufferRequest, NodeRequest, PasteBufferOpts, Request, Response};
use crate::tui::command::BufferCommand;
use crate::vt::capture::CaptureSpec;

pub(super) async fn command(tx: &Tx, ctx: &Context, command: BufferCommand) -> Result<(), String> {
    match command {
        BufferCommand::ListBuffers { .. } => buffer_req(tx, BufferRequest::List).await,
        BufferCommand::SetBuffer {
            name,
            append,
            rename,
            data,
        } => set_buffer(tx, name, append, rename, data).await,
        BufferCommand::ShowBuffer { name } => buffer_req(tx, BufferRequest::Get { name }).await,
        BufferCommand::DeleteBuffer { name } => {
            buffer_req(tx, BufferRequest::Delete { name }).await
        }
        BufferCommand::LoadBuffer { name, path } => load_buffer(tx, name, &path).await,
        BufferCommand::SaveBuffer { name, append, path } => {
            save_buffer(tx, name, append, &path).await
        }
        BufferCommand::PasteBuffer { opts } => paste_buffer(tx, ctx, opts).await,
        BufferCommand::CapturePane {
            target,
            print,
            buffer,
            spec,
        } => capture_pane(tx, target_pane(ctx, target)?, print, buffer, spec).await,
    }
}

/// `set-buffer`: stores `data` in a buffer (appended with `append`), or renames it with `rename`.
async fn set_buffer(
    tx: &Tx,
    name: Option<String>,
    append: bool,
    rename: Option<String>,
    data: Option<String>,
) -> Result<(), String> {
    match (rename, data) {
        (Some(to), None) => buffer_req(tx, BufferRequest::Rename { name, to }).await,
        (Some(_), Some(_)) => Err("set-buffer: -n takes no data".to_string()),
        (None, Some(data)) => buffer_req(tx, BufferRequest::Set { name, data, append }).await,
        (None, None) => Err("set-buffer: no data given".to_string()),
    }
}

/// `load-buffer`: a file becomes a paste buffer.
async fn load_buffer(tx: &Tx, name: Option<String>, path: &str) -> Result<(), String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("load-buffer: {path}: {e}"))?;
    buffer_req(
        tx,
        BufferRequest::Set {
            name,
            data,
            append: false,
        },
    )
    .await
}

/// `paste-buffer`: the pane defaults to the hook's pane.
async fn paste_buffer(tx: &Tx, ctx: &Context, mut opts: PasteBufferOpts) -> Result<(), String> {
    if opts.pane.is_empty() {
        opts.pane = hook_pane(ctx)?;
    }
    buffer_req(tx, BufferRequest::Paste(opts)).await
}

/// `capture-pane`: the text is shown (`print`, not kept here) or stored in buffer `buffer`.
async fn capture_pane(
    tx: &Tx,
    name: String,
    print: bool,
    buffer: Option<String>,
    spec: CaptureSpec,
) -> Result<(), String> {
    let text = match request(tx, Request::Node(NodeRequest::Capture { name, spec })).await? {
        Response::Capture(text) => text,
        other => return Err(format!("unexpected response to capture-pane: {other:?}")),
    };
    if print {
        return Ok(());
    }
    buffer_req(
        tx,
        BufferRequest::Set {
            name: buffer,
            data: text,
            append: false,
        },
    )
    .await
}

/// `save-buffer`: a paste buffer is written to a file, appended with `append`.
async fn save_buffer(
    tx: &Tx,
    name: Option<String>,
    append: bool,
    path: &str,
) -> Result<(), String> {
    if path == "-" {
        return Err("save-buffer: - (stdout) works only from the command line".to_string());
    }
    let Response::Buffer(buffer) =
        request(tx, Request::Buffer(BufferRequest::Get { name })).await?
    else {
        return Err("save-buffer: unexpected response".to_string());
    };
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .and_then(|mut file| file.write_all(buffer.data.as_bytes()))
        .map_err(|e| format!("save-buffer: {path}: {e}"))
}
