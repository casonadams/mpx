use super::Commands;
use super::panes::{default_node_name, pid_label, resolve_name};
use super::resolve::{Resolved, parse_env_pairs};
use super::sessions::ensure_client;
use crate::client::Client;
use crate::protocol::{LayoutPreset, NewWindowOpts, Side, SpawnOpts};

/// Arguments of `mpx new-window`.
pub struct NewWindowRequest {
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub pty: bool,
    pub focus: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
}

/// Arguments of `mpx resize-pane`.
pub struct ResizeRequest {
    pub target: Option<String>,
    pub side: Option<Side>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub amount: Option<u16>,
}

/// Direction of the first set `-L/-R/-U/-D` flag.
pub fn side_from_flags(left: bool, right: bool, up: bool, down: bool) -> Option<Side> {
    [
        (left, Side::Left),
        (right, Side::Right),
        (up, Side::Up),
        (down, Side::Down),
    ]
    .into_iter()
    .find(|(set, _)| *set)
    .map(|(_, side)| side)
}

pub async fn run(cmd: Commands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        Commands::NewWindow {
            name,
            cwd,
            env,
            pty,
            detach,
            command,
            args,
        } => {
            let req = NewWindowRequest {
                name,
                cwd,
                env,
                pty,
                focus: !detach,
                command,
                args,
            };
            new_window(r, req).await
        }
        Commands::KillWindow { index } => {
            let mut client = ensure_client(r).await?;
            client
                .kill_window(index)
                .await
                .map_err(|e| format!("kill-window error: {e}"))?;
            Ok(0)
        }
        Commands::RenameWindow { index, name } => {
            let mut client = ensure_client(r).await?;
            client
                .rename_window(index, &name)
                .await
                .map_err(|e| format!("rename-window error: {e}"))?;
            Ok(0)
        }
        Commands::SelectWindow { index } => {
            let mut client = ensure_client(r).await?;
            client
                .select_window(index)
                .await
                .map_err(|e| format!("select-window error: {e}"))?;
            Ok(0)
        }
        Commands::LastWindow => {
            let mut client = ensure_client(r).await?;
            client
                .last_window()
                .await
                .map_err(|e| format!("last-window error: {e}"))?;
            Ok(0)
        }
        Commands::LastPane => {
            let mut client = ensure_client(r).await?;
            client
                .focus_last()
                .await
                .map_err(|e| format!("last-pane error: {e}"))?;
            Ok(0)
        }
        Commands::SwapPane { source, target } => swap(r, &source, &target).await,
        Commands::BreakPane { name } => {
            let mut client = ensure_client(r).await?;
            client
                .break_pane(&name)
                .await
                .map_err(|e| format!("break-pane error: {e}"))?;
            Ok(0)
        }
        Commands::SelectLayout { preset } => select_layout(r, preset.as_deref()).await,
        Commands::ResizePane {
            target,
            left,
            right,
            up,
            down,
            width,
            height,
            amount,
        } => {
            let req = ResizeRequest {
                target,
                side: side_from_flags(left, right, up, down),
                width,
                height,
                amount,
            };
            resize(r, req).await
        }
        _ => Err("command is not routed".to_string()),
    }
}

fn shell_command(command: Option<String>, args: Vec<String>) -> (String, Vec<String>) {
    match command {
        Some(cmd) => (cmd, args),
        None => (
            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()),
            Vec::new(),
        ),
    }
}

async fn new_window(r: &Resolved, req: NewWindowRequest) -> Result<i32, String> {
    let (command, args) = shell_command(req.command, req.args);
    let node_name = req
        .name
        .clone()
        .unwrap_or_else(|| default_node_name(&command));
    let mut client = ensure_client(r).await?;
    let info = client
        .new_window(NewWindowOpts {
            name: req.name,
            spawn: SpawnOpts {
                name: node_name,
                command,
                args,
                pty: req.pty,
                cwd: req.cwd,
                env: parse_env_pairs(&req.env)?,
                remain_on_exit: None,
                tmux_compat: false,
            },
            focus: req.focus,
        })
        .await
        .map_err(|e| format!("new-window error: {e}"))?;
    println!(
        "Window created: '{}' (pid: {})",
        info.name,
        pid_label(info.pid)
    );
    Ok(0)
}

async fn swap(r: &Resolved, source: &str, target: &str) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let a = resolve_name(&mut client, Some(source)).await?;
    let b = resolve_name(&mut client, Some(target)).await?;
    client
        .swap_pane(&a, &b)
        .await
        .map_err(|e| format!("swap-pane error: {e}"))?;
    Ok(0)
}

async fn select_layout(r: &Resolved, preset: Option<&str>) -> Result<i32, String> {
    let preset = match preset {
        Some(name) => {
            Some(LayoutPreset::parse(name).ok_or_else(|| format!("unknown layout: {name}"))?)
        }
        None => None,
    };
    let mut client = ensure_client(r).await?;
    client
        .select_layout(preset)
        .await
        .map_err(|e| format!("select-layout error: {e}"))?;
    Ok(0)
}

async fn pane_size(client: &mut Client, name: &str) -> Result<(u16, u16), String> {
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    let (rects, _, _) = client.layout(w, h).await?;
    rects
        .iter()
        .find(|(n, _, _)| n == name)
        .map(|(_, rect, _)| (rect.width, rect.height))
        .ok_or_else(|| format!("can't find pane: {name}"))
}

async fn resize(r: &Resolved, req: ResizeRequest) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let name = resolve_name(&mut client, req.target.as_deref()).await?;
    if req.width.is_some() || req.height.is_some() {
        let (cur_w, cur_h) = pane_size(&mut client, &name).await?;
        let cols = req.width.unwrap_or(cur_w);
        let rows = req.height.unwrap_or(cur_h);
        client
            .resize_pane(&name, cols, rows)
            .await
            .map_err(|e| format!("resize-pane error: {e}"))?;
        return Ok(0);
    }
    let side = req
        .side
        .ok_or_else(|| "resize-pane needs -L, -R, -U, -D, or -x/-y".to_string())?;
    let viewport = crossterm::terminal::size().unwrap_or((80, 24));
    client
        .resize_split(&name, side, req.amount.unwrap_or(1), viewport)
        .await
        .map_err(|e| format!("resize-pane error: {e}"))?;
    Ok(0)
}
