use clap::Parser;
use tracing_subscriber::EnvFilter;

use mpx::cli::Cli;

/// True when the binary was invoked as `tmux` (symlink or copy in the shim directory).
fn is_tmux_invocation(argv0: Option<&String>) -> bool {
    argv0
        .and_then(|a| std::path::Path::new(a).file_name())
        .is_some_and(|name| name == "tmux" || name == "tmux.exe")
}

#[tokio::main]
async fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if is_tmux_invocation(argv.first()) {
        let code = mpx::tmux_shim::run(argv[1..].to_vec()).await;
        std::process::exit(code);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match mpx::cli::run(cli).await {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
