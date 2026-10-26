use clap::Parser;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use mpx::cli::{Cli, Commands};
use mpx::client::Client;
use mpx::daemon::run_daemon;
use mpx::ipc::default_endpoint;
use mpx::protocol::StreamEvent;

async fn ensure_client(endpoint: &str, session: &str) -> Result<Client, String> {
    if let Ok(client) = Client::connect(endpoint).await {
        return Ok(client);
    }

    // Attempt to auto-start daemon in background
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let _ = std::process::Command::new(current_exe)
        .arg("daemon")
        .arg("--session")
        .arg(session)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to auto-spawn mpx daemon: {}", e))?;

    // Retry connection up to 2 seconds
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(client) = Client::connect(endpoint).await {
            return Ok(client);
        }
    }

    Err(format!("Could not connect to mpx daemon at '{}'", endpoint))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let endpoint = cli
        .endpoint
        .unwrap_or_else(|| default_endpoint(&cli.session));

    match cli.command {
        Commands::Daemon => {
            run_daemon(&endpoint).await?;
        }
        Commands::Run {
            name,
            pty,
            cwd,
            command,
            args,
        } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let node_name = name.unwrap_or_else(|| {
                std::path::Path::new(&command)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "task".to_string())
            });

            let info = client
                .spawn(node_name, command, args, pty, cwd)
                .await
                .map_err(|e| format!("Run error: {}", e))?;

            println!(
                "Node '{}' started (pid: {}, pty: {})",
                info.name,
                info.pid.map(|p| p.to_string()).unwrap_or("-".into()),
                info.pty
            );
        }
        Commands::List => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let list = client.list().await.map_err(|e| format!("List error: {}", e))?;

            if list.is_empty() {
                println!("No active stream nodes in session '{}'", cli.session);
                return Ok(());
            }

            println!(
                "{:<16} {:<8} {:<8} {:<14} {:<30}",
                "NAME", "PID", "TYPE", "STATUS", "COMMAND"
            );
            println!("{}", "-".repeat(80));
            for node in list {
                let node_type = if node.pty { "pty" } else { "task" };
                let pid_str = node.pid.map(|p| p.to_string()).unwrap_or("-".into());
                println!(
                    "{:<16} {:<8} {:<8} {:<14} {:<30}",
                    node.name, pid_str, node_type, node.status, node.command
                );
            }
        }
        Commands::Logs {
            name,
            follow,
            lines,
        } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let history = client.logs(&name, lines).await.map_err(|e| format!("Logs error: {}", e))?;
            print!("{}", history);

            if follow {
                use std::io::Write;
                let _ = client
                    .stream_logs(Some(name), |event| match event {
                        StreamEvent::Output { data, .. } => {
                            print!("{}", data);
                            let _ = std::io::stdout().flush();
                            true
                        }
                        StreamEvent::Exit { code, .. } => {
                            eprintln!("\n[Process exited with code {:?}]", code);
                            false
                        }
                        _ => true,
                    })
                    .await;
            }
        }
        Commands::Send { name, data } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let payload = if data.ends_with('\n') {
                data
            } else {
                format!("{}\n", data)
            };
            client
                .send_input(&name, &payload)
                .await
                .map_err(|e| format!("Send error: {}", e))?;
            println!("Input sent to '{}'", name);
        }
        Commands::Kill { name } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            client.kill(&name).await.map_err(|e| format!("Kill error: {}", e))?;
            println!("Node '{}' terminated", name);
        }
        Commands::Wait { name } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let code = client.wait(&name).await.map_err(|e| format!("Wait error: {}", e))?;
            if let Some(c) = code {
                std::process::exit(c);
            }
        }
    }

    Ok(())
}
