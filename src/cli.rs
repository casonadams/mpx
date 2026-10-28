use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod agents;
mod panes;
pub mod render;
mod resolve;
mod sessions;
mod windows;

pub use resolve::{
    Resolved, nested_attach_refused, parse_agent_state, parse_env_pairs, resolve,
    resolve_idempotency_key, wait_condition, wait_exit_code,
};
pub use sessions::{RestoreOutcome, apply_key, daemon_args, restore_session, run_cli};

/// Process entry point behind the `mpx` binary: tmux multicall, tracing, clap parsing and dispatch.
/// Returns the process exit code.
pub fn main_entry(argv: Vec<String>) -> i32 {
    match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime.block_on(entry(argv)),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// True when the binary was invoked as `tmux` (symlink or copy in the shim directory).
fn is_tmux_invocation(argv0: Option<&String>) -> bool {
    argv0
        .and_then(|a| std::path::Path::new(a).file_name())
        .is_some_and(|name| name == "tmux" || name == "tmux.exe")
}

async fn entry(argv: Vec<String>) -> i32 {
    if is_tmux_invocation(argv.first()) {
        return crate::tmux_shim::run(argv[1..].to_vec()).await;
    }
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
    /// Session name (default: $MPX_SESSION, then "main")
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

        /// Export TMUX/TMUX_PANE and the tmux shim to the process
        #[arg(long = "tmux-compat")]
        tmux_compat: bool,

        /// Command to execute, then its arguments; options after the command are its own
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

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

        /// Wait until the agent reaches this state: needs_input, working, idle, exited
        #[arg(long)]
        state: Option<String>,
    },

    /// Snapshot the 2D virtual terminal screen of a node
    Screen {
        /// Name of the node
        name: String,
    },

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

        /// Export TMUX/TMUX_PANE and the tmux shim to the process
        #[arg(long = "tmux-compat")]
        tmux_compat: bool,

        /// Do not focus the new pane (tmux -d)
        #[arg(short = 'd', long = "detach", alias = "no-focus")]
        detach: bool,

        /// Command to execute, then its arguments; options after the command are its own
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },

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

    /// Switch to the previously focused pane
    #[command(name = "last-pane")]
    LastPane,

    /// Focus a specific pane
    Focus {
        /// Name of the pane to focus
        name: String,
    },

    /// Swap the positions of two panes
    #[command(name = "swap-pane")]
    SwapPane {
        /// Source pane (tmux -s; `-s` is the global session flag here)
        #[arg(long = "source")]
        source: String,

        #[arg(short = 't', long = "target")]
        target: String,
    },

    /// Move a pane into its own window
    #[command(name = "break-pane")]
    BreakPane {
        /// Pane to break out
        name: String,
    },

    /// Apply a layout preset (even-horizontal, even-vertical, main-vertical, main-horizontal, tiled); none cycles
    #[command(name = "select-layout")]
    SelectLayout { preset: Option<String> },

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

    /// Manage inter-agent virtual pipes
    Pipe {
        #[command(subcommand)]
        command: PipeCommands,
    },

    /// Agent sessions: `mpx agents` opens the TUI on the agent view; `--json` lists live and resumable agents
    Agents {
        /// Print {live, resumable} as JSON
        #[arg(long)]
        json: bool,

        #[command(subcommand)]
        command: Option<AgentCommands>,
    },

    /// Report a harness hook event (reads JSON on stdin); `print-config claude` prints the settings JSON
    Hook {
        /// Harness name (claude, omp) or `print-config`
        harness: String,

        /// Event name for harnesses that pass it as an argument (omp), or the harness for print-config
        event: Option<String>,
    },

    /// Launch stdio Model Context Protocol (MCP) server for AI agent harnesses
    Mcp,
}

#[derive(Subcommand, Debug)]
pub enum PipeCommands {
    /// Create a new virtual pipe
    Create {
        /// Name of the pipe
        name: String,
        /// Maximum capacity in bytes
        #[arg(long)]
        capacity: Option<usize>,
    },
    /// Write data to a virtual pipe
    Write {
        /// Name of the pipe
        name: String,
        /// Content data string
        data: String,
    },
    /// Read messages from a virtual pipe
    Read {
        /// Name of the pipe
        name: String,
        /// Only messages with a sequence number after this one
        #[arg(long)]
        since: Option<u64>,
        /// Daemon-side cursor name; reads resume where the consumer left off
        #[arg(long)]
        consumer: Option<String>,
        /// Block up to this many milliseconds for a message
        #[arg(long = "wait")]
        wait_ms: Option<u64>,
        /// Maximum number of messages
        #[arg(long)]
        limit: Option<usize>,
        /// Print {"messages": [...], "next": n}
        #[arg(long)]
        json: bool,
    },
    /// List active virtual pipes
    List {
        #[arg(long)]
        json: bool,
    },
    /// Stream messages as they are written
    Subscribe {
        /// Name of the pipe
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentCommands {
    /// Launch a harness (claude, omp, codex, ...) in a new window or split
    New {
        /// Harness name
        harness: String,
        /// Resume this session id
        #[arg(long)]
        resume: Option<String>,
        /// Split the focused pane instead of opening a window
        #[arg(long)]
        split: bool,
        /// Enable agent teams (tmux compatibility for teammates)
        #[arg(long)]
        teams: bool,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Node name
        #[arg(long)]
        name: Option<String>,
    },
    /// Resume a stored session by id prefix in a new window
    Resume {
        /// Session id or unique prefix
        prefix: String,
    },
}
