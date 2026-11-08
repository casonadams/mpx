//! Paste buffers and `capture-pane`. The daemon owns the buffers, so the CLI and TUI share them;
//! files and stdin are read and written here, where the paths mean something.
use std::io::Write;

use clap::Subcommand;

use super::panes::resolve_name;
use super::resolve::Resolved;
use super::sessions::daemon_call;
use crate::format::{DEFAULT_BUFFER_FORMAT, buffer_vars, expand};
use crate::protocol::PasteBufferOpts;
use crate::vt::capture::{CaptureLine, CaptureSpec};

#[derive(Subcommand, Debug)]
pub enum BufferCommands {
    /// Capture a pane's screen and history as text (tmux capture-pane)
    #[command(name = "capture-pane", alias = "capturep")]
    CapturePane {
        /// Pane to capture (defaults to the focused pane)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Print the text instead of storing it in a paste buffer
        #[arg(short = 'p')]
        print: bool,

        /// Keep escape sequences for colors and attributes
        #[arg(short = 'e')]
        escapes: bool,

        /// Join wrapped lines and keep trailing spaces
        #[arg(short = 'J')]
        join: bool,

        /// First line: 0 is the first visible line, -N is N lines back; `-` is the history start
        #[arg(short = 'S', allow_hyphen_values = true)]
        start: Option<String>,

        /// Last line, numbered as for -S; `-` is the last visible line
        #[arg(short = 'E', allow_hyphen_values = true)]
        end: Option<String>,

        /// Paste buffer to store the text in (default: a new buffer)
        #[arg(short = 'b')]
        buffer: Option<String>,
    },

    #[command(flatten)]
    Store(BufferStoreCommands),

    #[command(flatten)]
    Files(BufferFileCommands),

    #[command(flatten)]
    Paste(BufferPasteCommands),
}

/// The commands that set, show and delete paste buffers.
#[derive(Subcommand, Debug)]
pub enum BufferStoreCommands {
    /// Store text in a paste buffer; `-n` renames the buffer (tmux set-buffer)
    #[command(name = "set-buffer", alias = "setb")]
    Set {
        /// Buffer to set (default: a new buffer, or the newest one with -a)
        #[arg(short = 'b')]
        buffer: Option<String>,

        /// Append the text to the buffer instead of replacing it
        #[arg(short = 'a')]
        append: bool,

        /// Rename the buffer given by -b (or the newest one) to this name
        #[arg(short = 'n', conflicts_with = "data")]
        rename: Option<String>,

        /// Text to store
        #[arg(allow_hyphen_values = true)]
        data: Option<String>,
    },

    /// Print a paste buffer (tmux show-buffer)
    #[command(name = "show-buffer", alias = "showb")]
    Show {
        #[arg(short = 'b')]
        buffer: Option<String>,
    },

    /// Delete a paste buffer (tmux delete-buffer)
    #[command(name = "delete-buffer", alias = "deleteb")]
    Delete {
        #[arg(short = 'b')]
        buffer: Option<String>,
    },
}

/// The commands that load and save paste buffers through files.
#[derive(Subcommand, Debug)]
pub enum BufferFileCommands {
    /// Load a file, or stdin with `-`, into a paste buffer (tmux load-buffer)
    #[command(name = "load-buffer", alias = "loadb")]
    LoadBuffer {
        #[arg(short = 'b')]
        buffer: Option<String>,

        /// File to load, or `-` for stdin
        path: String,
    },

    /// Write a paste buffer to a file, or stdout with `-` (tmux save-buffer)
    #[command(name = "save-buffer", alias = "saveb")]
    SaveBuffer {
        #[arg(short = 'b')]
        buffer: Option<String>,

        /// Append to the file instead of replacing it
        #[arg(short = 'a')]
        append: bool,

        /// File to write, or `-` for stdout
        path: String,
    },
}

/// The commands that paste buffers into a pane and list them.
#[derive(Subcommand, Debug)]
pub enum BufferPasteCommands {
    /// Paste a paste buffer into a pane (tmux paste-buffer)
    #[command(name = "paste-buffer", alias = "pasteb")]
    PasteBuffer {
        #[arg(short = 'b')]
        buffer: Option<String>,

        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Delete the buffer after pasting
        #[arg(short = 'd')]
        delete: bool,

        /// Bracket the paste even when the program did not ask for bracketed paste
        #[arg(short = 'p')]
        bracketed: bool,

        /// Paste line feeds as they are, not as carriage returns
        #[arg(short = 'r')]
        raw: bool,

        /// Replace each line feed with this text
        #[arg(long = "separator", allow_hyphen_values = true)]
        separator: Option<String>,
    },

    /// List paste buffers, newest first (tmux list-buffers)
    #[command(name = "list-buffers", alias = "lsb")]
    ListBuffers {
        /// Format of each line: #{buffer_name}, #{buffer_size}, #{buffer_sample}
        #[arg(short = 'F', allow_hyphen_values = true)]
        format: Option<String>,
    },
}

/// A `-S` or `-E` value: `-` is the start of the history for `-S` and the last visible line for
/// `-E`; a number counts from the first visible line, negative numbers into the history.
pub fn capture_line(value: &str, end: bool) -> Result<CaptureLine, String> {
    match value {
        "-" if end => Ok(CaptureLine::Last),
        "-" => Ok(CaptureLine::First),
        number => number
            .parse()
            .map(CaptureLine::Line)
            .map_err(|_| format!("capture-pane: invalid line '{number}'")),
    }
}

/// The capture `capture-pane` asks the daemon for, from its flags.
pub fn capture_spec(
    start: Option<&str>,
    end: Option<&str>,
    escapes: bool,
    join: bool,
) -> Result<CaptureSpec, String> {
    Ok(CaptureSpec {
        start: start.map(|s| capture_line(s, false)).transpose()?,
        end: end.map(|e| capture_line(e, true)).transpose()?,
        escapes,
        join,
    })
}

pub async fn run(cmd: BufferCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        BufferCommands::CapturePane {
            target,
            print,
            escapes,
            join,
            start,
            end,
            buffer,
        } => {
            let spec = capture_spec(start.as_deref(), end.as_deref(), escapes, join)?;
            capture_pane(r, target.as_deref(), print, spec, buffer.as_deref()).await
        }
        BufferCommands::Store(cmd) => run_store(cmd, r).await,
        BufferCommands::Files(cmd) => match cmd {
            BufferFileCommands::LoadBuffer { buffer, path } => {
                load_buffer(r, buffer.as_deref(), &path).await
            }
            BufferFileCommands::SaveBuffer {
                buffer,
                append,
                path,
            } => save_buffer(r, buffer.as_deref(), append, &path).await,
        },
        BufferCommands::Paste(cmd) => run_paste(cmd, r).await,
    }
}

/// Runs `set-buffer`, `show-buffer` or `delete-buffer`.
async fn run_store(cmd: BufferStoreCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        BufferStoreCommands::Set {
            buffer,
            append,
            rename,
            data,
        } => set_buffer(r, buffer.as_deref(), append, rename.as_deref(), data).await,
        BufferStoreCommands::Show { buffer } => show_buffer(r, buffer.as_deref()).await,
        BufferStoreCommands::Delete { buffer } => delete_buffer(r, buffer.as_deref()).await,
    }
}

/// Runs `paste-buffer` or `list-buffers`.
async fn run_paste(cmd: BufferPasteCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        BufferPasteCommands::PasteBuffer {
            buffer,
            target,
            delete,
            bracketed,
            raw,
            separator,
        } => {
            daemon_call(r, "paste-buffer", async move |c| {
                let pane = resolve_name(c, target.as_deref()).await?;
                let opts = PasteBufferOpts {
                    name: buffer,
                    pane,
                    delete,
                    bracketed,
                    raw,
                    separator,
                };
                c.paste_buffer(opts).await
            })
            .await?;
            Ok(0)
        }
        BufferPasteCommands::ListBuffers { format } => list_buffers(r, format.as_deref()).await,
    }
}

async fn capture_pane(
    r: &Resolved,
    target: Option<&str>,
    print: bool,
    spec: CaptureSpec,
    buffer: Option<&str>,
) -> Result<i32, String> {
    let text = daemon_call(r, "capture-pane", async move |c| {
        let name = resolve_name(c, target).await?;
        let text = c.capture_pane(&name, spec).await?;
        if !print {
            c.set_buffer(buffer, &text, false).await?;
        }
        Ok(text)
    })
    .await?;
    if print {
        write_stdout(&text)?;
    }
    Ok(0)
}

async fn set_buffer(
    r: &Resolved,
    buffer: Option<&str>,
    append: bool,
    rename: Option<&str>,
    data: Option<String>,
) -> Result<i32, String> {
    match (rename, data) {
        (Some(to), None) => {
            daemon_call(r, "set-buffer", async move |c| {
                c.rename_buffer(buffer, to).await
            })
            .await?
        }
        (Some(_), Some(_)) => return Err("set-buffer: -n takes no data".to_string()),
        (None, Some(data)) => {
            daemon_call(r, "set-buffer", async move |c| {
                c.set_buffer(buffer, &data, append).await
            })
            .await?
        }
        (None, None) => return Err("set-buffer: no data given".to_string()),
    }
    Ok(0)
}

async fn show_buffer(r: &Resolved, buffer: Option<&str>) -> Result<i32, String> {
    let found = daemon_call(r, "show-buffer", async move |c| c.get_buffer(buffer).await).await?;
    write_stdout(&found.data)?;
    Ok(0)
}

async fn delete_buffer(r: &Resolved, buffer: Option<&str>) -> Result<i32, String> {
    daemon_call(r, "delete-buffer", async move |c| {
        c.delete_buffer(buffer).await
    })
    .await?;
    Ok(0)
}

async fn load_buffer(r: &Resolved, buffer: Option<&str>, path: &str) -> Result<i32, String> {
    let data = if path == "-" {
        std::io::read_to_string(std::io::stdin()).map_err(|e| format!("load-buffer: stdin: {e}"))?
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("load-buffer: {path}: {e}"))?
    };
    daemon_call(r, "load-buffer", async move |c| {
        c.set_buffer(buffer, &data, false).await
    })
    .await?;
    Ok(0)
}

async fn save_buffer(
    r: &Resolved,
    buffer: Option<&str>,
    append: bool,
    path: &str,
) -> Result<i32, String> {
    let found = daemon_call(r, "save-buffer", async move |c| c.get_buffer(buffer).await).await?;
    if path == "-" {
        write_stdout(&found.data)?;
    } else {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .map_err(|e| format!("save-buffer: {path}: {e}"))?;
        file.write_all(found.data.as_bytes())
            .map_err(|e| format!("save-buffer: {path}: {e}"))?;
    }
    Ok(0)
}

async fn list_buffers(r: &Resolved, format: Option<&str>) -> Result<i32, String> {
    let buffers = daemon_call(r, "list-buffers", async move |c| c.list_buffers().await).await?;
    let format = format.unwrap_or(DEFAULT_BUFFER_FORMAT);
    for buffer in &buffers {
        println!("{}", expand(format, &buffer_vars(buffer)));
    }
    Ok(0)
}

fn write_stdout(text: &str) -> Result<(), String> {
    let mut out = std::io::stdout();
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("write error: {e}"))
}
