use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod buffers;
mod clients;
mod environment;
mod hooks;
mod panes;
pub mod render;
mod resolve;
mod sessions;
mod windows;
pub use buffers::{capture_line, capture_spec};

pub use resolve::{
    DEFAULT_SESSION, LEGACY_DEFAULT_SESSION, Resolved, nested_attach_refused, next_free_session,
    parse_env_pairs, resolve, resolve_idempotency_key, wait_condition, wait_exit_code,
};
pub use sessions::{RestoreOutcome, apply_key, daemon_args, restore_session, run_cli};
pub use windows::{
    PaneCommands, PaneMoveCommands, ResizeCommands, RespawnCommands, WindowCommands,
    WindowLinkCommands, WindowOrderCommands,
};

/// Process entry point behind the `mpx` binary: tracing, clap parsing and dispatch.
/// Returns the process exit code.
pub fn main_entry(argv: Vec<String>) -> i32 {
    // One thread: mpx's work is actors passing messages, and with a worker pool every message
    // between them became a cross-thread wakeup. A flood cost the daemon and the TUI 40% more CPU.
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(entry(argv)),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn entry(argv: Vec<String>) -> i32 {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            let code = e.exit_code();
            let _ = e.print();
            return code;
        }
    };
    match run_cli(cli).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "mpx",
    about = "Simple, performant process and stream multiplexer",
    version
)]
pub struct Cli {
    /// Session name (default: $MPX_SESSION, then "0")
    #[arg(short, long, global = true)]
    pub session: Option<String>,

    /// Daemon endpoint (default: $MPX_ENDPOINT when -s is not given, else the session socket)
    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    /// Config file to load instead of the default location (tmux -f; give it before the command)
    #[arg(short = 'f', long = "config", value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Durable state directory of the daemon (default: the session's directory under the state dir)
    #[arg(long = "state-dir", global = true, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,

    /// Start the session empty instead of restoring its saved layout (only when its daemon is not already running)
    #[arg(long = "no-restore", global = true)]
    pub no_restore: bool,

    /// Run the mutating request of this command at most once per KEY (safe retries)
    #[arg(long = "idempotency-key", global = true, value_name = "KEY")]
    pub idempotency_key: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the mpx background daemon
    Daemon,

    /// Recreate the session's windows, panes and agents from its last snapshot
    Restore,

    /// Print an mpx config.toml fragment converted from a tmux config (tmux.conf); writes no files
    #[command(name = "import-tmux")]
    ImportTmux {
        /// tmux config to convert (default: ~/.tmux.conf, then ~/.config/tmux/tmux.conf)
        file: Option<PathBuf>,
    },

    /// Stop the session's daemon (tmux kill-server)
    #[command(name = "kill-server", aliases = ["stop", "kill-session"])]
    KillServer,

    /// Attach the interactive full-screen TUI to a session
    Attach {
        /// Session to attach (tmux -t)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Attach even when running inside an mpx pane
        #[arg(long)]
        force: bool,

        /// Detach the session's other clients first (tmux attach -d)
        #[arg(short = 'd', long = "detach-others")]
        detach_others: bool,

        /// Like -d, and on Unix send SIGHUP to the other clients' parent processes (tmux attach -x)
        #[arg(short = 'x', long = "exclusive")]
        exclusive: bool,

        /// Attach read-only: keys and paste do not reach the panes (tmux attach -r)
        #[arg(short = 'r', long = "read-only")]
        read_only: bool,
    },

    /// Create a new session, named by the next free number unless -s gives a name (tmux new-session)
    #[command(name = "new-session", alias = "new")]
    NewSession {
        /// Create the session without attaching to it
        #[arg(short = 'd', long = "detach")]
        detach: bool,

        /// Session group target: refused, sessions cannot share windows (tmux new-session -t)
        #[arg(short = 't', long = "group-target")]
        target: Option<String>,

        /// Attach even when running inside an mpx pane
        #[arg(long)]
        force: bool,
    },

    /// List sessions and their panes and windows
    #[command(name = "list-sessions", alias = "ls")]
    ListSessions {
        #[arg(long)]
        json: bool,
    },

    /// List panes (stream nodes) of the session
    #[command(name = "list-panes", aliases = ["list", "lsp"])]
    ListPanes {
        #[arg(long)]
        json: bool,
    },

    /// Exit 0 when the session's daemon answers, 1 otherwise
    HasSession {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },

    /// Run a task or command in a managed stream node
    Run {
        /// Name of the stream node (defaults to command name)
        #[arg(short, long)]
        name: Option<String>,

        /// Run inside an interactive PTY instead of a raw pipe
        #[arg(long)]
        pty: bool,

        /// Working directory
        #[arg(long)]
        cwd: Option<String>,

        /// Extra environment variable K=V (repeatable)
        #[arg(long = "env", value_name = "K=V")]
        env: Vec<String>,

        /// Keep the pane on screen after the process exits
        #[arg(long)]
        remain: bool,

        /// Command to execute, then its arguments; options after the command are its own
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

    /// Read logs, send input, wait on, capture and kill nodes
    #[command(flatten)]
    Nodes(panes::NodeCommands),

    /// Split an existing pane and run a new command (`-h` is side by side as in tmux; use --help)
    #[command(disable_help_flag = true)]
    Split {
        /// Print help
        #[arg(long, action = clap::ArgAction::Help)]
        help: Option<bool>,

        /// Target pane to split (defaults to the focused pane)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        /// Name of the new stream node
        #[arg(short, long)]
        name: Option<String>,

        /// Split direction: h (side by side) or v (stacked)
        #[arg(long, default_value = "h")]
        direction: String,

        /// Shorthand for --direction h (tmux -h)
        #[arg(short = 'h', conflicts_with = "vertical")]
        horizontal: bool,

        /// Shorthand for --direction v (tmux -v)
        #[arg(short = 'v')]
        vertical: bool,

        /// New pane size as a percentage of the target (tmux -l N%)
        #[arg(short = 'l', long = "size")]
        percent: Option<u8>,

        /// Extra environment variable K=V (repeatable)
        #[arg(long = "env", value_name = "K=V")]
        env: Vec<String>,

        /// Run inside an interactive PTY
        #[arg(long)]
        pty: bool,

        /// Do not focus the new pane (tmux -d)
        #[arg(short = 'd', long = "detach", alias = "no-focus")]
        detach: bool,

        /// Keep the window zoomed if it was zoomed (tmux -Z)
        #[arg(short = 'Z')]
        keep_zoom: bool,

        /// Command to execute, then its arguments; options after the command are its own
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

    /// Window commands (new-window, kill-window, rename-window, select-window, last-window)
    #[command(flatten)]
    Window(WindowCommands),

    /// Pane focus, marks, breaks and layouts (select-pane, last-pane, break-pane, select-layout)
    #[command(flatten)]
    Pane(PaneCommands),

    /// Moving panes (swap-pane, join-pane)
    #[command(flatten)]
    PaneMove(PaneMoveCommands),

    /// Resizing a pane (resize-pane)
    #[command(flatten)]
    Resize(ResizeCommands),

    /// Restarting processes (respawn-pane, respawn-window)
    #[command(flatten)]
    Respawn(RespawnCommands),

    /// Window order and unsupported window commands (rotate-window, swap-window, move-window, ...)
    #[command(flatten)]
    WindowOrder(WindowOrderCommands),

    /// Window links and window sizes (link-window, unlink-window, resize-window)
    #[command(flatten)]
    WindowLink(WindowLinkCommands),

    /// Focus a specific pane
    Focus {
        /// Name of the pane to focus
        name: String,
    },

    /// Get status and metadata for a specific node
    Status {
        /// Name of the node
        name: String,
    },

    /// Show calculated layout geometry for current terminal dimensions
    Layout {
        /// Screen width in columns (defaults to current terminal width or 80)
        #[arg(long)]
        width: Option<u16>,

        /// Screen height in rows (defaults to current terminal height or 24)
        #[arg(long)]
        height: Option<u16>,
    },

    /// Rename a session (tmux rename-session)
    #[command(name = "rename-session", alias = "rename")]
    RenameSession {
        /// Target session (defaults to current session)
        #[arg(short = 't', long = "target")]
        target: Option<String>,

        name: String,
    },

    /// Paste buffers and `capture-pane`
    #[command(flatten)]
    Buffers(buffers::BufferCommands),

    /// The session environment: set-environment and show-environment
    #[command(flatten)]
    Environment(environment::EnvCommands),

    /// Daemon-side commands: run-shell, if-shell, wait-for, set-hook and show-hooks
    #[command(flatten)]
    Hooks(hooks::HookCommands),

    /// Attached clients: list them, detach or redraw them, and switch them to another session
    #[command(flatten)]
    Clients(clients::ClientCommands),
}
