use clap::Subcommand;
use super::Commands;

use super::panes::{
    command_and_args, current_layout, daemon_call, daemon_result, keyed_call, keyed_client,
    keyed_on_target, pid_label, resolve_name, resolve_optional, spawn_opts, terminal_size,
};
use super::resolve::Resolved;
use super::sessions::ensure_client;
use crate::client::Client;
use crate::layout::WindowTab;
use crate::protocol::{LayoutPreset, NewWindowOpts, Side};

/// Window and pane-layout commands, flattened into `mpx` next to the pane commands.
#[derive(Subcommand, Debug)]
pub enum WindowCommands {
    /// Create a window with a new pane (tmux new-window)
    #[command(name = "new-window")]
    NewWindow {
        /// Window label
        #[arg(short, long)]
        name: Option<String>,

        /// Working directory
        #[arg(short = 'c', long = "cwd")]
        cwd: Option<String>,

        /// Extra environment variable K=V (repeatable)
        #[arg(long = "env", value_name = "K=V")]
        env: Vec<String>,

        /// Run inside an interactive PTY
        #[arg(long)]
        pty: bool,

        /// Do not focus the new window (tmux -d)
        #[arg(short = 'd', long = "detach")]
        detach: bool,

        /// Command to execute (defaults to the shell), then its arguments; options after the command are its own
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

    /// Close a window (defaults to the active window)
    #[command(name = "kill-window")]
    KillWindow {
        #[arg(short = 't', long = "target")]
        index: Option<usize>,
    },

    /// Rename a window
    #[command(name = "rename-window")]
    RenameWindow {
        #[arg(short = 't', long = "target")]
        index: Option<usize>,

        name: String,
    },

    /// Select a window by index
    #[command(name = "select-window")]
    SelectWindow {
        #[arg(short = 't', long = "target")]
        index: usize,
    },

    /// Switch to the previously active window
    #[command(name = "last-window")]
    LastWindow,
}

/// Pane focus, marks and layouts: the pane commands that keep every pane where it is.
#[derive(Subcommand, Debug)]
pub enum PaneCommands {
    /// Switch to the previously focused pane
    #[command(name = "last-pane")]
    LastPane {
        /// Keep the window zoomed if it was zoomed (tmux -Z)
        #[arg(short = 'Z')]
        keep_zoom: bool,
    },

    /// Select, mark, or unmark a pane
    #[command(name = "select-pane", alias = "selectp")]
    SelectPane {
        /// Target pane (defaults to active pane)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Mark target pane (toggle if already marked)
        #[arg(short = 'm')]
        mark: bool,

        /// Clear the marked pane
        #[arg(short = 'M')]
        clear_mark: bool,

        /// Disable input to the target pane: keys typed or sent to it are dropped (tmux -d)
        #[arg(short = 'd')]
        disable_input: bool,

        /// Enable input to the target pane again (tmux -e)
        #[arg(short = 'e')]
        enable_input: bool,

        /// Keep the window zoomed if it was zoomed (tmux -Z)
        #[arg(short = 'Z')]
        keep_zoom: bool,
    },

    /// Move a pane into its own window
    #[command(name = "break-pane")]
    BreakPane {
        /// Pane to break out
        name: String,
    },

    /// Apply a layout preset (even-horizontal, even-vertical, main-vertical, main-vertical-mirrored,
    /// main-horizontal, main-horizontal-mirrored, tiled); none cycles
    #[command(name = "select-layout")]
    SelectLayout {
        preset: Option<String>,
        /// Spread the panes evenly, keeping their arrangement (tmux -E)
        #[arg(short = 'E', conflicts_with = "preset")]
        spread: bool,
    },
}

/// Moving panes: swap two panes, or join one into another pane's window.
#[derive(Subcommand, Debug)]
pub enum PaneMoveCommands {
    /// Swap the positions of two panes (defaults to swapping active pane with marked pane)
    #[command(name = "swap-pane")]
    SwapPane {
        /// Source pane (tmux -s; defaults to marked pane or active pane)
        #[arg(long = "source")]
        source: Option<String>,

        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Keep the window zoomed if it was zoomed (tmux -Z)
        #[arg(short = 'Z')]
        keep_zoom: bool,
    },

    /// Join a pane into another pane's window, splitting the destination (tmux join-pane, move-pane)
    #[command(
        name = "join-pane",
        aliases = ["joinp", "move-pane", "movep"],
        disable_help_flag = true
    )]
    JoinPane {
        /// Print help
        #[arg(long, action = clap::ArgAction::Help)]
        help: Option<bool>,

        /// Pane to move (defaults to the marked pane)
        #[arg(long = "source")]
        source: Option<String>,

        /// Pane to join it to (defaults to the active pane)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Join side by side (tmux -h); the default joins stacked
        #[arg(short = 'h', conflicts_with = "vertical")]
        horizontal: bool,

        /// Join stacked (tmux -v, the default)
        #[arg(short = 'v')]
        vertical: bool,

        /// Keep the active pane active (tmux -d)
        #[arg(short = 'd', long = "detach")]
        detach: bool,
    },
}

/// Resizing a pane, or zooming its window.
#[derive(Subcommand, Debug)]
pub enum ResizeCommands {
    /// Resize a pane: -L/-R/-U/-D move its border by AMOUNT cells; -x/-y set the size
    #[command(name = "resize-pane")]
    ResizePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        #[arg(short = 'L')]
        left: bool,

        #[arg(short = 'R')]
        right: bool,

        #[arg(short = 'U')]
        up: bool,

        #[arg(short = 'D')]
        down: bool,

        /// Absolute width in columns
        #[arg(short = 'x')]
        width: Option<u16>,

        /// Absolute height in rows
        #[arg(short = 'y')]
        height: Option<u16>,

        /// Cells to move (default 1)
        amount: Option<u16>,

        /// Zoom the window of the target pane, or unzoom it (tmux -Z); takes no other resize flag
        #[arg(
            short = 'Z',
            conflicts_with_all = ["left", "right", "up", "down", "width", "height", "amount"]
        )]
        zoom: bool,
    },
}

/// Restarting a pane's process or every pane of a window.
#[derive(Subcommand, Debug)]
pub enum RespawnCommands {
    /// Restart a pane's process in place (tmux respawn-pane)
    #[command(name = "respawn-pane", alias = "respawnp")]
    RespawnPane {
        /// Pane to restart (defaults to the active pane)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Kill the running process first (tmux -k)
        #[arg(short = 'k')]
        kill: bool,

        /// Working directory for the new process (tmux -c)
        #[arg(short = 'c', long = "cwd")]
        cwd: Option<String>,

        /// Command to run, then its arguments (default: the pane's own command)
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

    /// Restart every pane of a window (tmux respawn-window)
    #[command(name = "respawn-window", alias = "respawnw")]
    RespawnWindow {
        /// Window index (defaults to the active window)
        #[arg(short = 't', long = "target")]
        target: Option<usize>,

        /// Kill running processes first (tmux -k)
        #[arg(short = 'k')]
        kill: bool,

        /// Working directory for the new processes (tmux -c)
        #[arg(short = 'c', long = "cwd")]
        cwd: Option<String>,

        /// Command to run in each pane, then its arguments (default: each pane's own command)
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },
}

/// Window order: rotating panes, exchanging and moving windows, and the window commands that are
/// not supported.
#[derive(Subcommand, Debug)]
pub enum WindowOrderCommands {
    /// Rotate the panes of the active window through its layout slots (tmux rotate-window)
    #[command(name = "rotate-window", alias = "rotatew")]
    RotateWindow {
        /// Rotate downward (tmux -D)
        #[arg(short = 'D', conflicts_with = "up")]
        down: bool,

        /// Rotate upward (tmux -U, the default)
        #[arg(short = 'U')]
        up: bool,

        /// Keep the window zoomed if it was zoomed (tmux -Z)
        #[arg(short = 'Z')]
        keep_zoom: bool,
    },

    /// Exchange two windows by index (tmux swap-window)
    #[command(name = "swap-window", alias = "swapw")]
    SwapWindow {
        /// Window to swap (defaults to the active window)
        #[arg(long = "source")]
        source: Option<usize>,

        /// Window to swap it with
        #[arg(short = 't', long = "target")]
        target: usize,
    },

    /// Move a window to another index (tmux move-window)
    #[command(name = "move-window", alias = "movew")]
    MoveWindow {
        /// Window to move (defaults to the active window)
        #[arg(long = "source")]
        source: Option<usize>,

        /// Index to move it to
        #[arg(short = 't', long = "target")]
        target: usize,
    },
}

/// Window commands mpx refuses: tmux features it has no equivalent for.
#[derive(Subcommand, Debug)]
pub enum UnsupportedWindowCommands {
    /// Not supported: a window cannot be linked into another place (tmux link-window)
    #[command(name = "link-window", alias = "linkw")]
    LinkWindow {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Not supported: windows are never linked, so there is nothing to unlink (tmux unlink-window)
    #[command(name = "unlink-window", alias = "unlinkw")]
    UnlinkWindow {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Not supported: a window has no size of its own (tmux resize-window)
    #[command(name = "resize-window", alias = "resizew")]
    ResizeWindow {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
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

/// Runs a window or pane-layout command of `cli::Commands`, routed to its group.
pub async fn run(cmd: Commands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        Commands::Window(cmd) => run_window(cmd, r).await,
        Commands::Pane(cmd) => run_pane(cmd, r).await,
        Commands::PaneMove(cmd) => run_pane_move(cmd, r).await,
        Commands::Resize(cmd) => run_resize(cmd, r).await,
        Commands::Respawn(cmd) => run_respawn(cmd, r).await,
        Commands::WindowOrder(cmd) => run_window_order(cmd, r).await,
        Commands::Unsupported(cmd) => run_unsupported(cmd),
        _ => Err("command is not routed".to_string()),
    }
}

async fn run_window(cmd: WindowCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        WindowCommands::NewWindow {
            name,
            cwd,
            env,
            pty,
            detach,
            argv,
        } => new_window(
            r,
            NewWindowRequest {
                name,
                cwd,
                env,
                pty,
                focus: !detach,
                argv,
            },
        )
        .await,
        WindowCommands::KillWindow { index } => kill_window(r, index).await,
        WindowCommands::RenameWindow { index, name } => rename_window(r, index, &name).await,
        WindowCommands::SelectWindow { index } => select_window(r, index).await,
        WindowCommands::LastWindow => {
            daemon_call(r, "last-window", async |c| c.last_window().await).await
        }
    }
}

/// Pane focus, marks, breaks and layouts.
async fn run_pane(cmd: PaneCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        PaneCommands::LastPane { keep_zoom } => last_pane(r, keep_zoom).await,
        PaneCommands::SelectPane {
            target,
            mark,
            clear_mark,
            disable_input,
            enable_input,
            keep_zoom,
        } => {
            let input = (disable_input || enable_input).then_some(disable_input);
            let flags = SelectPaneFlags {
                mark,
                clear_mark,
                input,
                keep_zoom,
            };
            select_pane(r, target.as_deref(), flags).await
        }
        PaneCommands::BreakPane { name } => break_pane(r, &name).await,
        PaneCommands::SelectLayout { preset, spread } => {
            select_layout(r, preset.as_deref(), spread).await
        }
    }
}

async fn run_pane_move(cmd: PaneMoveCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        PaneMoveCommands::SwapPane {
            source,
            target,
            keep_zoom,
        } => swap(r, source.as_deref(), target.as_deref(), keep_zoom).await,
        PaneMoveCommands::JoinPane {
            source,
            target,
            horizontal,
            detach,
            ..
        } => join_pane(r, source, target, horizontal, detach).await,
    }
}

async fn run_resize(cmd: ResizeCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        ResizeCommands::ResizePane {
            target,
            left,
            right,
            up,
            down,
            width,
            height,
            amount,
            zoom,
        } => {
            if zoom {
                return zoom_pane(r, target).await;
            }
            let req = ResizeRequest {
                target,
                side: side_from_flags(left, right, up, down),
                width,
                height,
                amount,
            };
            resize(r, req).await
        }
    }
}

async fn run_respawn(cmd: RespawnCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        RespawnCommands::RespawnPane {
            target,
            kill,
            cwd,
            argv,
        } => respawn_pane(r, target, kill, cwd, argv).await,
        RespawnCommands::RespawnWindow {
            target,
            kill,
            cwd,
            argv,
        } => respawn_window(r, target, kill, cwd, argv).await,
    }
}

async fn run_window_order(cmd: WindowOrderCommands, r: &Resolved) -> Result<i32, String> {
    match cmd {
        WindowOrderCommands::RotateWindow {
            down, keep_zoom, ..
        } => rotate_window(r, down, keep_zoom).await,
        WindowOrderCommands::SwapWindow { source, target } => swap_window(r, source, target).await,
        WindowOrderCommands::MoveWindow { source, target } => move_window(r, source, target).await,
    }
}

/// The refusal for a window command mpx does not support, as an error.
fn run_unsupported(cmd: UnsupportedWindowCommands) -> Result<i32, String> {
    let why = match cmd {
        UnsupportedWindowCommands::LinkWindow { .. } => {
            "link-window: not supported: a pane belongs to one window, so a window cannot be linked elsewhere"
        }
        UnsupportedWindowCommands::UnlinkWindow { .. } => {
            "unlink-window: not supported: windows are never linked, so there is nothing to unlink"
        }
        UnsupportedWindowCommands::ResizeWindow { .. } => {
            "resize-window: not supported: a window's size is set by the attached terminal"
        }
    };
    Err(why.to_string())
}

async fn kill_window(r: &Resolved, index: Option<usize>) -> Result<i32, String> {
    keyed_call(r, "kill-window", async |c| c.kill_window(index).await).await
}

async fn rename_window(r: &Resolved, index: Option<usize>, name: &str) -> Result<i32, String> {
    keyed_call(r, "rename-window", async |c| {
        c.rename_window(index, name).await
    })
    .await
}

async fn select_window(r: &Resolved, index: usize) -> Result<i32, String> {
    daemon_call(r, "select-window", async |c| c.select_window(index).await).await
}

async fn last_pane(r: &Resolved, keep_zoom: bool) -> Result<i32, String> {
    daemon_call(r, "last-pane", async |c| c.focus_last_with(keep_zoom).await).await
}

async fn break_pane(r: &Resolved, name: &str) -> Result<i32, String> {
    daemon_call(r, "break-pane", async |c| c.break_pane(name).await).await
}

fn shell_command(argv: Vec<String>) -> (String, Vec<String>) {
    command_and_args(argv).unwrap_or_else(|| {
        (
            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()),
            Vec::new(),
        )
    })
}

/// The command a respawn runs, or `None` to keep each pane's own command.
fn respawn_command(argv: Vec<String>) -> Option<Vec<String>> {
    (!argv.is_empty()).then_some(argv)
}

/// Arguments of `mpx new-window`.
pub struct NewWindowRequest {
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub pty: bool,
    pub focus: bool,
    pub argv: Vec<String>,
}

async fn new_window(r: &Resolved, req: NewWindowRequest) -> Result<i32, String> {
    let NewWindowRequest {
        name,
        cwd,
        env,
        pty,
        focus,
        argv,
    } = req;
    let (command, args) = shell_command(argv);
    let mut client = keyed_client(r).await?;
    let spawn = spawn_opts(name.clone(), command, args, pty, cwd, &env)?;
    let info = client
        .new_window(NewWindowOpts { name, spawn, focus })
        .await
        .map_err(|e| format!("new-window error: {e}"))?;
    println!(
        "Window created: '{}' (pid: {})",
        info.name,
        pid_label(info.pid)
    );
    Ok(0)
}

/// The flags of `select-pane`, bundled so the handler stays under the parameter budget.
struct SelectPaneFlags {
    mark: bool,
    clear_mark: bool,
    input: Option<bool>,
    keep_zoom: bool,
}

async fn select_pane(
    r: &Resolved,
    target: Option<&str>,
    flags: SelectPaneFlags,
) -> Result<i32, String> {
    let SelectPaneFlags {
        mark,
        clear_mark,
        input,
        keep_zoom,
    } = flags;
    let mut client = ensure_client(r).await?;
    if clear_mark {
        daemon_result(client.mark_pane(None, true).await, "select-pane")?;
    } else if mark {
        let name = resolve_optional(&mut client, target).await?;
        daemon_result(
            client.mark_pane(name.as_deref(), false).await,
            "select-pane",
        )?;
    } else if let Some(off) = input {
        let name = resolve_optional(&mut client, target).await?;
        daemon_result(
            client.input_off(name.as_deref(), off).await,
            "select-pane",
        )?;
    } else if let Some(target) = target {
        let name = resolve_name(&mut client, Some(target)).await?;
        daemon_result(client.focus_with(&name, keep_zoom).await, "select-pane")?;
    }
    Ok(0)
}

async fn swap(
    r: &Resolved,
    source: Option<&str>,
    target: Option<&str>,
    keep_zoom: bool,
) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let a = resolve_optional(&mut client, source).await?;
    let b = resolve_optional(&mut client, target).await?;
    daemon_result(
        client
            .swap_pane_targets(a.as_deref(), b.as_deref(), keep_zoom)
            .await,
        "swap-pane",
    )?;
    Ok(0)
}

async fn select_layout(r: &Resolved, preset: Option<&str>, spread: bool) -> Result<i32, String> {
    if spread {
        return daemon_call(r, "select-layout", async |c| c.spread_layout().await).await;
    }
    let preset = preset
        .map(|name| LayoutPreset::parse(name).ok_or_else(|| format!("unknown layout: {name}")))
        .transpose()?;
    daemon_call(r, "select-layout", async |c| c.select_layout(preset).await).await
}

async fn pane_size(client: &mut Client, name: &str) -> Result<(u16, u16), String> {
    let (rects, _, _) = current_layout(client).await?;
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
        daemon_result(client.resize_pane(&name, cols, rows).await, "resize-pane")?;
        return Ok(0);
    }
    let side = req
        .side
        .ok_or_else(|| "resize-pane needs -L, -R, -U, -D, or -x/-y".to_string())?;
    daemon_result(
        client
            .resize_split(&name, side, req.amount.unwrap_or(1), terminal_size())
            .await,
        "resize-pane",
    )?;
    Ok(0)
}

async fn zoom_pane(r: &Resolved, target: Option<String>) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let name = resolve_optional(&mut client, target.as_deref()).await?;
    daemon_result(client.zoom(name.as_deref(), None).await, "resize-pane")?;
    Ok(0)
}

/// The window tabs of the layout the daemon reports.
async fn window_tabs(client: &mut Client) -> Result<Vec<WindowTab>, String> {
    Ok(current_layout(client).await?.2)
}

/// Index of the active window, from the layout the daemon reports.
async fn active_window(client: &mut Client) -> Result<usize, String> {
    let tabs = window_tabs(client).await?;
    let active = tabs.into_iter().find(|tab| tab.is_active);
    active
        .map(|tab| tab.index)
        .ok_or("no active window".to_string())
}

/// The pane marked in the session, from the layout the daemon reports.
async fn marked_pane(client: &mut Client) -> Result<String, String> {
    let tabs = window_tabs(client).await?;
    tabs.iter()
        .find_map(|t| t.marked_pane.clone())
        .ok_or_else(|| "no marked pane".to_string())
}

async fn join_pane(
    r: &Resolved,
    source: Option<String>,
    target: Option<String>,
    side_by_side: bool,
    detach: bool,
) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let src = match source {
        Some(s) => resolve_name(&mut client, Some(&s)).await?,
        None => marked_pane(&mut client).await?,
    };
    let dst = resolve_name(&mut client, target.as_deref()).await?;
    daemon_result(
        client.join_pane(&src, &dst, side_by_side, detach).await,
        "join-pane",
    )?;
    Ok(0)
}

async fn respawn_pane(
    r: &Resolved,
    target: Option<String>,
    kill: bool,
    cwd: Option<String>,
    argv: Vec<String>,
) -> Result<i32, String> {
    keyed_on_target(r, target.as_deref(), "respawn-pane", async |c, name| {
        c.respawn(&name, kill, cwd, respawn_command(argv)).await
    })
    .await
}

async fn respawn_window(
    r: &Resolved,
    target: Option<usize>,
    kill: bool,
    cwd: Option<String>,
    argv: Vec<String>,
) -> Result<i32, String> {
    keyed_call(r, "respawn-window", async |c| {
        c.respawn_window(target, kill, cwd, respawn_command(argv))
            .await
    })
    .await
}

async fn rotate_window(r: &Resolved, down: bool, keep_zoom: bool) -> Result<i32, String> {
    daemon_call(r, "rotate-window", async |c| {
        c.rotate_window(down, keep_zoom).await
    })
    .await
}

/// The window a swap or move acts on: `source`, or the active window when none is given.
async fn source_window(client: &mut Client, source: Option<usize>) -> Result<usize, String> {
    match source {
        Some(index) => Ok(index),
        None => active_window(client).await,
    }
}

async fn swap_window(r: &Resolved, source: Option<usize>, target: usize) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let from = source_window(&mut client, source).await?;
    daemon_result(client.swap_window(from, target).await, "swap-window")?;
    Ok(0)
}

async fn move_window(r: &Resolved, source: Option<usize>, target: usize) -> Result<i32, String> {
    let mut client = ensure_client(r).await?;
    let from = source_window(&mut client, source).await?;
    daemon_result(client.move_window(from, target).await, "move-window")?;
    Ok(0)
}
