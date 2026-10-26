use clap::Parser;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use mpx::cli::{Cli, Commands};
use mpx::cli::PipeCommands;
use mpx::mcp::McpServer;
use mpx::client::Client;
use mpx::daemon::run_daemon;
use mpx::ipc::default_endpoint;
use mpx::protocol::StreamEvent;
use mpx::layout::Direction;

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

    let command = cli.command.unwrap_or(Commands::Attach);
    match command {
        Commands::Daemon => {
            run_daemon(&endpoint).await?;
        }
        Commands::Attach => {
            let _ = ensure_client(&endpoint, &cli.session).await?;
            mpx::tui::run_tui(&endpoint, &cli.session)
                .await
                .map_err(|e| format!("TUI error: {}", e))?;
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
        Commands::Screen { name } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let screen = client.screen(&name).await.map_err(|e| format!("Screen error: {}", e))?;
            print!("{}", screen);
        }
        Commands::Split {
            target,
            name,
            direction,
            pty,
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

            let dir = if direction.to_lowercase().starts_with('v') {
                Direction::Vertical
            } else {
                Direction::Horizontal
            };

            let info = client
                .split(target, node_name, command, args, pty, dir)
                .await
                .map_err(|e| format!("Split error: {}", e))?;

            println!(
                "Split pane created: '{}' (pid: {}, pty: {})",
                info.name,
                info.pid.map(|p| p.to_string()).unwrap_or("-".into()),
                info.pty
            );
        }
        Commands::Layout { width, height } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
            let w = width.unwrap_or(term_w);
            let h = height.unwrap_or(term_h);

            let (rects, _dividers, _tabs) = client.layout(w, h).await.map_err(|e| format!("Layout error: {}", e))?;
            println!("Workspace Layout ({}x{}):", w, h);
            println!("{:<16} {:<10} {:<10} {:<10}", "PANE", "RECT (x,y)", "SIZE (wxh)", "FOCUSED");
            println!("{}", "-".repeat(50));
            for (name, rect, focused) in rects {
                let pos = format!("{},{}", rect.x, rect.y);
                let size = format!("{}x{}", rect.width, rect.height);
                let foc = if focused { "yes" } else { "no" };
                println!("{:<16} {:<10} {:<10} {:<10}", name, pos, size, foc);
            }
        }
        Commands::Pipe { command: pipe_cmd } => {
            let mut client = ensure_client(&endpoint, &cli.session).await?;
            match pipe_cmd {
                PipeCommands::Create { name, capacity } => {
                    client.create_pipe(&name, capacity).await.map_err(|e| format!("Pipe create error: {}", e))?;
                    println!("Virtual pipe '{}' created", name);
                }
                PipeCommands::Write { name, data } => {
                    client.write_pipe(&name, &data).await.map_err(|e| format!("Pipe write error: {}", e))?;
                }
                PipeCommands::Read { name } => {
                    let content = client.read_pipe(&name).await.map_err(|e| format!("Pipe read error: {}", e))?;
                    print!("{}", content);
                }
                PipeCommands::List => {
                    let pipes = client.list_pipes().await.map_err(|e| format!("Pipe list error: {}", e))?;
                    if pipes.is_empty() {
                        println!("No virtual pipes in session '{}'", cli.session);
                    } else {
                        println!("PIPES ({})", pipes.len());
                        println!("{}", "-".repeat(30));
                        for p in pipes {
                            println!("{}", p);
                        }
                    }
                }
            }
        }
        Commands::Mcp => {
            let _ = ensure_client(&endpoint, &cli.session).await?;
            let server = McpServer::new(endpoint, cli.session);
            server.run_stdio().await.map_err(|e| format!("MCP error: {}", e))?;
        }
    }

    Ok(())
}
