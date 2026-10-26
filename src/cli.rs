use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "mpx", about = "Simple, performant process and stream multiplexer", version)]
pub struct Cli {
    #[arg(short, long, default_value = "main", global = true)]
    pub session: String,

    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the mpx background daemon
    Daemon,

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

        /// Command to execute
        #[arg(required = true)]
        command: String,

        /// Arguments for the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// List active stream nodes and their status
    #[command(alias = "ls")]
    List,

    /// Read or stream logs from a node
    Logs {
        /// Name of the node
        name: String,

        /// Follow stream live
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show from history
        #[arg(short = 'n', long)]
        lines: Option<usize>,
    },

    /// Send input to a running node's stdin
    Send {
        /// Name of the node
        name: String,

        /// Input data string
        data: String,
    },

    /// Terminate a running node
    Kill {
        /// Name of the node
        name: String,
    },

    /// Wait for a node to exit and return its exit code
    Wait {
        /// Name of the node
        name: String,
    },

    /// Snapshot the 2D virtual terminal screen of a node
    Screen {
        /// Name of the node
        name: String,
    },

    /// Split an existing pane and run a new command
    Split {
        /// Target pane to split (defaults to active focused pane)
        #[arg(short, long)]
        target: Option<String>,

        /// Name of the new stream node
        #[arg(short, long)]
        name: Option<String>,

        /// Split direction: horizontal (h) or vertical (v)
        #[arg(short, long, default_value = "h")]
        direction: String,

        /// Run inside an interactive PTY
        #[arg(long)]
        pty: bool,

        /// Command to execute
        #[arg(required = true)]
        command: String,

        /// Arguments for the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
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
}
