use clap::Subcommand;

use super::Commands;
use super::render::{FollowStep, follow_step};
use super::resolve::{Resolved, parse_env_pairs, wait_condition, wait_exit_code};
use super::sessions::{apply_key, ensure_client};
use super::windows;
use crate::client::{Client, LayoutTuple};
use crate::layout::Direction;
use crate::node::process::file_name;
use crate::protocol::{NodeInfo, SpawnOpts, SplitOpts, StreamEvent};

/// Arguments of `mpx split`, grouped so the handler stays under the parameter budget.
pub struct SplitRequest {
    pub target: Option<String>,
    pub name: Option<String>,
    pub vertical: bool,
    pub percent: Option<u8>,
    pub env: Vec<String>,
    pub pty: bool,
    pub detach: bool,
    pub keep_zoom: bool,
    pub command: String,
    pub args: Vec<String>,
}

/// Arguments of `mpx run`.
pub struct RunRequest {
    pub name: Option<String>,
    pub pty: bool,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub remain: bool,
    pub command: String,
    pub args: Vec<String>,
}

/// The node commands of the CLI, flattened into `Commands`.
#[derive(Subcommand, Debug)]
pub enum NodeCommands {
    /// Read or stream logs from a node
    Logs {
        /// Name of the node
        name: String,

        /// Follow stream live
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show from history
        #[arg(short = 'n', long, visible_alias = "tail")]
        lines: Option<usize>,

        /// Render scrollback as plain text (no escape sequences)
        #[arg(long)]
        plain: bool,

        /// Only output after this byte offset (the `next` value of a previous call)
        #[arg(long)]
        since: Option<u64>,

        /// Print {"data": ..., "next": ...}
        #[arg(long)]
        json: bool,
    },

    /// Send raw input (plus a newline) to a running node's stdin
    Send {
        /// Name of the node
        name: String,

        /// Input data string
        data: String,
    },

    #[command(flatten)]
    Input(NodeInputCommands),

    #[command(flatten)]
    Control(NodeControlCommands),
}

/// The commands that send keys or text to a pane.
#[derive(Subcommand, Debug)]
pub enum NodeInputCommands {
    /// Send keys to a pane (tmux send-keys); key names such as Enter, C-c, or literal text with -l
    #[command(name = "send-keys")]
    SendKeys {
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Send the operands as literal text
        #[arg(short = 'l', long = "literal")]
        literal: bool,

        /// Keys or text
        #[arg(allow_hyphen_values = true)]
        keys: Vec<String>,
    },

    /// Paste text into a pane; `-` reads stdin
    Paste {
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Press Enter after the paste
        #[arg(long)]
        submit: bool,

        /// Text to paste, or `-` for stdin
        #[arg(allow_hyphen_values = true)]
        text: String,
    },
}

/// The commands that end, wait on or snapshot a node.
#[derive(Subcommand, Debug)]
pub enum NodeControlCommands {
    /// Terminate a running node
    Kill {
        /// Name of the node
        name: String,
    },

    /// Wait for a node: exit code (default), idle output, screen match, or agent state
    Wait {
        /// Name of the node
        name: String,

        /// Give up after this many seconds (exit 124)
        #[arg(long)]
        timeout: Option<u64>,

        /// Wait until no output for this many milliseconds
        #[arg(long)]
        idle: Option<u64>,

        /// Wait until the screen or output matches this regex
        #[arg(long = "match")]
        pattern: Option<String>,
    },
    /// Snapshot the 2D virtual terminal screen of a node
    Screen {
        /// Name of the node
        name: String,
    },
}

pub fn default_node_name(command: &str) -> String {
    file_name(command).unwrap_or_else(|| "task".to_string())
}

/// The command and its arguments from an `argv` positional, or `None` when it is empty.
pub fn command_and_args(argv: Vec<String>) -> Option<(String, Vec<String>)> {
    let mut argv = argv.into_iter();
    let command = argv.next()?;
    Some((command, argv.collect()))
}

pub fn pid_label(pid: Option<u32>) -> String {
    pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())
}

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Target key for a tmux-style target: strips `session:` and `window.` prefixes, keeping `%id` or a node name.
fn pane_key(target: &str) -> &str {
    let last = target.rsplit(':').next().unwrap_or(target);
    match last.rsplit_once('.') {
        Some((_, pane)) if pane.starts_with('%') => pane,
        _ => last,
    }
}

/// Node name for a tmux-style target: `%id` is looked up, anything else is taken as a name.
/// No target means the focused pane.
pub async fn resolve_name(client: &mut Client, target: Option<&str>) -> Result<String, String> {
    let Some(target) = target else {
        return focused_name(client).await;
    };
    let key = pane_key(target);
    let Some(id) = key.strip_prefix('%') else {
        return Ok(key.to_string());
    };
    let id: u32 = id.parse().map_err(|_| format!("can't find pane: {key}"))?;
    let nodes = client.list().await?;
    nodes
        .into_iter()
        .find(|n: &NodeInfo| n.id == id)
        .map(|n| n.name)
        .ok_or_else(|| format!("can't find pane: {key}"))
}

pub async fn focused_name(client: &mut Client) -> Result<String, String> {
    let (rects, _, _) = current_layout(client).await?;
    rects
        .into_iter()
        .find(|r| r.2)
        .map(|r| r.0)
        .ok_or_else(|| "no focused pane".to_string())
}

/// The spawn request for a new node; its name defaults to the command's file name.
pub fn spawn_opts(
    name: Option<String>,
    command: String,
    args: Vec<String>,
    pty: bool,
    cwd: Option<String>,
    env: &[String],
) -> Result<SpawnOpts, String> {
    Ok(SpawnOpts {
        name: name.unwrap_or_else(|| default_node_name(&command)),
        command,
        args,
        pty,
        cwd,
        env: parse_env_pairs(env)?,
        remain_on_exit: None,
    })
}

/// `resolve_name` for an optional target: `None` stays `None` instead of meaning the focused pane.
pub async fn resolve_optional(
    client: &mut Client,
    target: Option<&str>,
) -> Result<Option<String>, String> {
    match target {
        Some(target) => resolve_name(client, Some(target)).await.map(Some),
        None => Ok(None),
    }
}

/// The terminal size the CLI lays panes out for when it has no size of its own.
pub fn terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

/// The layout the daemon reports for the current terminal size.
pub async fn current_layout(client: &mut Client) -> Result<LayoutTuple, String> {
    let (w, h) = terminal_size();
    client.layout(w, h).await
}

/// Maps a failed daemon call to `"<what> error: <e>"`.
pub fn daemon_result<T>(result: Result<T, String>, what: &str) -> Result<T, String> {
    result.map_err(|e| format!("{what} error: {e}"))
}

/// Runs one daemon request on a connected client; a failure reads `"<what> error: <e>"`.
pub async fn daemon_call(
    r: &Resolved,
    what: &str,
    op: impl AsyncFnOnce(&mut Client) -> Result<(), String>,
) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    daemon_result(op(&mut client).await, what)?;
    Ok(0)
}

/// A daemon client for a mutating command: the request's idempotency key is applied.
pub async fn keyed_client(r: &Resolved) -> Result<Client, String> {
    let mut client = ensure_client(r).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    Ok(client)
}

/// `daemon_call` for a mutating request: the request's idempotency key is applied first.
pub async fn keyed_call(
    r: &Resolved,
    what: &str,
    op: impl AsyncFnOnce(&mut Client) -> Result<(), String>,
) -> Result<i32, String> {
    let mut client = keyed_client(r).await?;
    daemon_result(op(&mut client).await, what)?;
    Ok(0)
}

/// Resolves a pane target, then runs one mutating request on that node with the idempotency key
/// applied. A failed lookup is returned as is; a failed request reads `"<what> error: <e>"`.
pub async fn keyed_on_target(
    r: &Resolved,
    target: Option<&str>,
    what: &str,
    op: impl AsyncFnOnce(&mut Client, String) -> Result<(), String>,
) -> Result<i32, String> {
    let mut client = keyed_client(r).await?;
    let name = resolve_name(&mut client, target).await?;
    daemon_result(op(&mut client, name).await, what)?;
    Ok(0)
}

/// Runs a node command: logs, input, waits, screen captures and kills.
async fn node_command(cmd: NodeCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        NodeCommands::Logs {
            name,
            follow,
            lines,
            plain,
            since,
            json,
        } => {
            logs(
                r,
                LogsRequest {
                    name,
                    follow,
                    lines,
                    plain,
                    since,
                    json,
                },
            )
            .await
        }
        NodeCommands::Send { name, data } => send(r, &name, &data).await,
        NodeCommands::Input(cmd) => node_input(cmd, r).await,
        NodeCommands::Control(cmd) => node_control(cmd, r).await,
    }
}

/// Runs `send-keys` or `paste`.
async fn node_input(cmd: NodeInputCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        NodeInputCommands::SendKeys {
            target,
            literal,
            keys,
        } => send_keys(r, target.as_deref(), literal, keys).await,
        NodeInputCommands::Paste {
            target,
            submit,
            text,
        } => paste(r, target.as_deref(), submit, &text).await,
    }
}

/// Runs `kill`, `wait` or `screen`.
async fn node_control(cmd: NodeControlCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        NodeControlCommands::Kill { name } => kill_node(r, &name).await,
        NodeControlCommands::Wait {
            name,
            timeout,
            idle,
            pattern,
        } => {
            wait(
                r,
                &name,
                timeout,
                idle,
                pattern.as_deref(),
            )
            .await
        }
        NodeControlCommands::Screen { name } => screen_node(r, &name).await,
    }
}

pub async fn run(cmd: Commands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        Commands::Run {
            name,
            pty,
            cwd,
            env,
            remain,
            argv,
        } => run_command(r, name, pty, cwd, env, remain, argv).await,
        Commands::Nodes(cmd) => node_command(cmd, r).await,
        Commands::Split {
            target,
            name,
            direction,
            vertical,
            percent,
            env,
            pty,
            detach,
            keep_zoom,
            argv,
            ..
        } => {
            let (command, args) =
                command_and_args(argv).ok_or_else(|| "no command given".to_string())?;
            let req = SplitRequest {
                target,
                name,
                vertical: vertical || direction.to_lowercase().starts_with('v'),
                percent,
                env,
                pty,
                detach,
                keep_zoom,
                command,
                args,
            };
            split(r, req).await
        }
        Commands::Focus { name } => focus_node(r, &name).await,
        Commands::Status { name } => status(r, &name).await,
        Commands::Layout { width, height } => layout(r, width, height).await,
        other => windows::run(other, r).await,
    }
}

async fn run_node(r: &Resolved, req: RunRequest) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let node_name = req.name.unwrap_or_else(|| default_node_name(&req.command));
    apply_key(&mut client, r.idempotency_key.as_deref());
    let info = client
        .spawn(SpawnOpts {
            name: node_name,
            command: req.command,
            args: req.args,
            pty: req.pty,
            cwd: req.cwd,
            env: parse_env_pairs(&req.env)?,
            remain_on_exit: req.remain.then_some(true),
        })
        .await
        .map_err(|e| format!("Run error: {e}"))?;
    println!(
        "Node '{}' started (pid: {}, pty: {})",
        info.name,
        pid_label(info.pid),
        info.pty
    );
    Ok(0)
}

async fn run_command(
    r: &Resolved,
    name: Option<String>,
    pty: bool,
    cwd: Option<String>,
    env: Vec<String>,
    remain: bool,
    argv: Vec<String>,
) -> Result<i32, String> {
    let (command, args) = command_and_args(argv).ok_or_else(|| "no command given".to_string())?;
    let req = RunRequest {
        name,
        pty,
        cwd,
        env,
        remain,
        command,
        args,
    };
    run_node(r, req).await
}

async fn kill_node(r: &Resolved, name: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    client
        .kill(name)
        .await
        .map_err(|e| format!("Kill error: {e}"))?;
    println!("Node '{name}' terminated");
    Ok(0)
}

async fn screen_node(r: &Resolved, name: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let screen = client
        .screen(name)
        .await
        .map_err(|e| format!("Screen error: {e}"))?;
    print!("{screen}");
    Ok(0)
}

async fn focus_node(r: &Resolved, name: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    client
        .focus(name)
        .await
        .map_err(|e| format!("Focus error: {e}"))?;
    println!("Focused pane '{name}'");
    Ok(0)
}

/// Arguments of `mpx logs`.
pub struct LogsRequest {
    pub name: String,
    pub follow: bool,
    pub lines: Option<usize>,
    pub plain: bool,
    pub since: Option<u64>,
    pub json: bool,
}

async fn logs(r: &Resolved, req: LogsRequest) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let (data, next) = client
        .logs(&req.name, req.lines, req.plain, req.since)
        .await
        .map_err(|e| format!("Logs error: {e}"))?;
    if req.json {
        println!("{}", serde_json::json!({ "data": data, "next": next }));
        return Ok(0);
    }
    print_flushed(&data);
    if req.follow {
        follow_logs(r, &req.name, next).await?;
    }
    Ok(0)
}

/// Streams `name` after byte offset `cursor` without gaps. A lag or a gap re-reads the log from the
/// last byte printed, then streams again from there.
async fn follow_logs(r: &Resolved, name: &str, mut cursor: u64) -> Result<(), String> {
    loop {
        let mut client = Client::connect(&r.endpoint).await?;
        let mut resync = false;
        client
            .stream_logs(Some(name.to_string()), Some(cursor), |event| match event {
                StreamEvent::Output {
                    data, offset, next, ..
                } => match follow_step(cursor, offset, next) {
                    FollowStep::Print => {
                        print_flushed(&data);
                        cursor = next;
                        true
                    }
                    FollowStep::Skip => true,
                    FollowStep::Resync => {
                        resync = true;
                        false
                    }
                },
                StreamEvent::Lagged => {
                    resync = true;
                    false
                }
                StreamEvent::Exit { code, .. } => {
                    match code {
                        Some(c) => eprintln!("\n[process exited with code {c}]"),
                        None => eprintln!("\n[process exited]"),
                    }
                    false
                }
                _ => true,
            })
            .await?;
        if !resync {
            return Ok(());
        }
        let mut reader = Client::connect(&r.endpoint).await?;
        let (data, next) = reader
            .logs(name, None, false, Some(cursor))
            .await
            .map_err(|e| format!("Logs error: {e}"))?;
        print_flushed(&data);
        cursor = next;
    }
}

fn print_flushed(data: &str) {
    use std::io::Write;
    print!("{data}");
    let _ = std::io::stdout().flush();
}

async fn send(r: &Resolved, name: &str, data: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    let payload = if data.ends_with('\n') {
        data.to_string()
    } else {
        format!("{data}\n")
    };
    client
        .send_input(name, &payload)
        .await
        .map_err(|e| format!("Send error: {e}"))?;
    println!("Input sent to '{name}'");
    Ok(0)
}

async fn send_keys(
    r: &Resolved,
    target: Option<&str>,
    literal: bool,
    keys: Vec<String>,
) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let name = resolve_name(&mut client, target).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    client
        .send_keys(&name, keys, literal)
        .await
        .map_err(|e| format!("send-keys error: {e}"))?;
    Ok(0)
}

async fn paste(
    r: &Resolved,
    target: Option<&str>,
    submit: bool,
    text: &str,
) -> Result<i32, String> {
    let text = if text == "-" {
        std::io::read_to_string(std::io::stdin()).map_err(|e| format!("Paste error: {e}"))?
    } else {
        text.to_string()
    };
    let mut client = ensure_client(r).await?;
    let name = resolve_name(&mut client, target).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    client
        .paste(&name, &text, submit)
        .await
        .map_err(|e| format!("Paste error: {e}"))?;
    Ok(0)
}

async fn wait(
    r: &Resolved,
    name: &str,
    timeout: Option<u64>,
    idle: Option<u64>,
    pattern: Option<&str>,
) -> Result<i32, String> {
    let condition = wait_condition(idle, pattern)?;
    let mut client = ensure_client(r).await?;
    let outcome = client
        .wait(name, condition.clone(), timeout.map(|s| s * 1000))
        .await
        .map_err(|e| format!("Wait error: {e}"))?;
    Ok(wait_exit_code(&condition, &outcome))
}

async fn split(r: &Resolved, req: SplitRequest) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let target = resolve_optional(&mut client, req.target.as_deref()).await?;
    let direction = if req.vertical {
        Direction::Vertical
    } else {
        Direction::Horizontal
    };
    apply_key(&mut client, r.idempotency_key.as_deref());
    let spawn = spawn_opts(
        req.name,
        req.command,
        req.args,
        req.pty,
        current_dir(),
        &req.env,
    )?;
    let info = client
        .split(SplitOpts {
            target,
            spawn,
            direction,
            percent: req.percent,
            focus: !req.detach,
            keep_zoom: req.keep_zoom,
        })
        .await
        .map_err(|e| format!("Split error: {e}"))?;
    println!(
        "Split pane created: '{}' (pid: {}, pty: {})",
        info.name,
        pid_label(info.pid),
        info.pty
    );
    Ok(0)
}

async fn status(r: &Resolved, name: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let info = client
        .status(name)
        .await
        .map_err(|e| format!("Status error: {e}"))?;
    println!("Node: {}", info.name);
    println!("ID: {}", info.pane_ref());
    println!("PID: {}", pid_label(info.pid));
    println!("PTY: {}", info.pty);
    println!("Status: {}", info.status_label());
    if !info.command.is_empty() {
        println!("Command: {}", info.command);
    }
    Ok(0)
}

async fn layout(r: &Resolved, width: Option<u16>, height: Option<u16>) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
    let w = width.unwrap_or(term_w);
    let h = height.unwrap_or(term_h);
    let (rects, _dividers, _tabs) = client
        .layout(w, h)
        .await
        .map_err(|e| format!("Layout error: {e}"))?;
    println!("Workspace Layout ({w}x{h}):");
    println!(
        "{:<16} {:<10} {:<10} {:<10}",
        "PANE", "RECT (x,y)", "SIZE (wxh)", "FOCUSED"
    );
    println!("{}", "-".repeat(50));
    for (name, rect, focused) in rects {
        let pos = format!("{},{}", rect.x, rect.y);
        let size = format!("{}x{}", rect.width, rect.height);
        let foc = if focused { "yes" } else { "no" };
        println!("{name:<16} {pos:<10} {size:<10} {foc:<10}");
    }
    Ok(0)
}
