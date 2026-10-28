use super::Commands;
use super::render::{FollowStep, follow_step};
use super::resolve::{Resolved, parse_env_pairs, wait_condition, wait_exit_code};
use super::sessions::{apply_key, ensure_client};
use super::windows;
use crate::agents::state_label;
use crate::client::Client;
use crate::layout::Direction;
use crate::protocol::{NodeInfo, SpawnOpts, SplitOpts, StreamEvent};
use crate::tmux_shim::pane_key;

/// Arguments of `mpx split`, grouped so the handler stays under the parameter budget.
pub struct SplitRequest {
    pub target: Option<String>,
    pub name: Option<String>,
    pub vertical: bool,
    pub percent: Option<u8>,
    pub env: Vec<String>,
    pub pty: bool,
    pub tmux_compat: bool,
    pub detach: bool,
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
    pub tmux_compat: bool,
    pub command: String,
    pub args: Vec<String>,
}

pub fn default_node_name(command: &str) -> String {
    std::path::Path::new(command)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "task".to_string())
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
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    let (rects, _, _) = client.layout(w, h).await?;
    rects
        .into_iter()
        .find(|r| r.2)
        .map(|r| r.0)
        .ok_or_else(|| "no focused pane".to_string())
}

pub async fn run(cmd: Commands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        Commands::Run {
            name,
            pty,
            cwd,
            env,
            remain,
            tmux_compat,
            argv,
        } => {
            let (command, args) =
                command_and_args(argv).ok_or_else(|| "no command given".to_string())?;
            let req = RunRequest {
                name,
                pty,
                cwd,
                env,
                remain,
                tmux_compat,
                command,
                args,
            };
            run_node(r, req).await
        }
        Commands::Logs {
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
        Commands::Send { name, data } => send(r, &name, &data).await,
        Commands::SendKeys {
            target,
            literal,
            keys,
        } => send_keys(r, target.as_deref(), literal, keys).await,
        Commands::Paste {
            target,
            submit,
            text,
        } => paste(r, target.as_deref(), submit, &text).await,
        Commands::Kill { name } => {
            let mut client = ensure_client(r).await?;
            apply_key(&mut client, r.idempotency_key.as_deref());
            client
                .kill(&name)
                .await
                .map_err(|e| format!("Kill error: {e}"))?;
            println!("Node '{name}' terminated");
            Ok(0)
        }
        Commands::Wait {
            name,
            timeout,
            idle,
            pattern,
            state,
        } => {
            wait(
                r,
                &name,
                timeout,
                idle,
                pattern.as_deref(),
                state.as_deref(),
            )
            .await
        }
        Commands::Screen { name } => {
            let mut client = ensure_client(r).await?;
            let screen = client
                .screen(&name)
                .await
                .map_err(|e| format!("Screen error: {e}"))?;
            print!("{screen}");
            Ok(0)
        }
        Commands::Split {
            target,
            name,
            direction,
            vertical,
            percent,
            env,
            pty,
            tmux_compat,
            detach,
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
                tmux_compat,
                detach,
                command,
                args,
            };
            split(r, req).await
        }
        Commands::Focus { name } => {
            let mut client = ensure_client(r).await?;
            client
                .focus(&name)
                .await
                .map_err(|e| format!("Focus error: {e}"))?;
            println!("Focused pane '{name}'");
            Ok(0)
        }
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
            tmux_compat: req.tmux_compat,
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
    state: Option<&str>,
) -> Result<i32, String> {
    let condition = wait_condition(idle, pattern, state)?;
    let mut client = ensure_client(r).await?;
    let outcome = client
        .wait(name, condition.clone(), timeout.map(|s| s * 1000))
        .await
        .map_err(|e| format!("Wait error: {e}"))?;
    Ok(wait_exit_code(&condition, &outcome))
}

async fn split(r: &Resolved, req: SplitRequest) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let target = match req.target.as_deref() {
        Some(t) => Some(resolve_name(&mut client, Some(t)).await?),
        None => None,
    };
    let node_name = req.name.unwrap_or_else(|| default_node_name(&req.command));
    let direction = if req.vertical {
        Direction::Vertical
    } else {
        Direction::Horizontal
    };
    apply_key(&mut client, r.idempotency_key.as_deref());
    let info = client
        .split(SplitOpts {
            target,
            spawn: SpawnOpts {
                name: node_name,
                command: req.command,
                args: req.args,
                pty: req.pty,
                cwd: current_dir(),
                env: parse_env_pairs(&req.env)?,
                remain_on_exit: None,
                tmux_compat: req.tmux_compat,
            },
            direction,
            percent: req.percent,
            focus: !req.detach,
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
    if let Some(agent) = &info.agent {
        println!("Agent: {} ({})", agent.harness, state_label(agent.state));
    }
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
