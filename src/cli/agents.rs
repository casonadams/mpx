use std::io::Read;

use super::AgentCommands;
use super::resolve::Resolved;
use super::sessions::{apply_key, ensure_client};
use crate::agents::{
    self, Harness, LaunchOptions, claude_settings_json, find_harness, find_unique_by_prefix,
};
use crate::config::Config;
use crate::layout::Direction;
use crate::protocol::{NewWindowOpts, SpawnOpts, SplitOpts};
use crate::tui::{StartView, run_tui};

const LIST_LIMIT: usize = 50;
const RESUME_SCAN_LIMIT: usize = 5000;

/// Arguments shared by `agents new` and `agents resume`.
pub struct LaunchRequest {
    pub harness: String,
    pub resume: Option<String>,
    pub split: bool,
    pub teams: bool,
    pub cwd: Option<String>,
    pub name: Option<String>,
}

pub async fn agents(
    r: &Resolved,
    json: bool,
    command: Option<AgentCommands>,
) -> Result<i32, String> {
    match command {
        Some(AgentCommands::New {
            harness,
            resume,
            split,
            teams,
            cwd,
            name,
        }) => {
            let req = LaunchRequest {
                harness,
                resume,
                split,
                teams,
                cwd,
                name,
            };
            launch(r, req).await
        }
        Some(AgentCommands::Resume { prefix }) => resume(r, &prefix).await,
        None if json => {
            let registry = agents::registry(&Config::load_or_default());
            let inventory = agents::inventory_json(&registry, LIST_LIMIT).await;
            println!(
                "{}",
                serde_json::to_string_pretty(&inventory).map_err(|e| e.to_string())?
            );
            Ok(0)
        }
        None => {
            ensure_client(r).await?;
            let _ = run_tui(&r.endpoint, &r.session, StartView::Agents).await;
            Ok(0)
        }
    }
}

async fn launch(r: &Resolved, req: LaunchRequest) -> Result<i32, String> {
    let registry = agents::registry(&Config::load_or_default());
    let harness = find_harness(&registry, &req.harness)
        .ok_or_else(|| format!("unknown harness '{}'", req.harness))?
        .clone();
    start(r, &harness, &req).await
}

async fn resume(r: &Resolved, prefix: &str) -> Result<i32, String> {
    let registry = agents::registry(&Config::load_or_default());
    let found = agents::scan_resumable(&registry, None, RESUME_SCAN_LIMIT);
    let session = find_unique_by_prefix(found, prefix)?;
    let harness = find_harness(&registry, &session.harness)
        .ok_or_else(|| format!("unknown harness '{}'", session.harness))?
        .clone();
    let req = LaunchRequest {
        harness: harness.name.clone(),
        resume: Some(session.id.clone()),
        split: false,
        teams: false,
        cwd: session.cwd.clone(),
        name: None,
    };
    start(r, &harness, &req).await
}

async fn start(r: &Resolved, harness: &Harness, req: &LaunchRequest) -> Result<i32, String> {
    let spec = agents::launch(
        harness,
        req.resume.as_deref(),
        LaunchOptions { teams: req.teams },
    );
    let (command, args) = spec
        .argv
        .split_first()
        .ok_or_else(|| format!("harness '{}' has no command", harness.name))?;
    let name = req
        .name
        .clone()
        .unwrap_or_else(|| agents::default_node_name(&spec));
    let spawn = SpawnOpts {
        name: name.clone(),
        command: command.clone(),
        args: args.to_vec(),
        pty: true,
        cwd: req.cwd.clone().or_else(current_dir),
        env: spec.env.clone(),
        remain_on_exit: None,
        tmux_compat: spec.tmux_compat,
    };
    let mut client = ensure_client(r).await?;
    apply_key(&mut client, r.idempotency_key.as_deref());
    let info = if req.split {
        client
            .split(SplitOpts {
                target: None,
                spawn,
                direction: Direction::Horizontal,
                percent: None,
                focus: true,
            })
            .await
    } else {
        client
            .new_window(NewWindowOpts {
                name: Some(name),
                spawn,
                focus: true,
            })
            .await
    }
    .map_err(|e| format!("Agent launch error: {e}"))?;
    match &spec.session_id {
        Some(id) => println!(
            "Launched {} as '{}' (session {id})",
            harness.name, info.name
        ),
        None => println!("Launched {} as '{}'", harness.name, info.name),
    }
    Ok(0)
}

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// `mpx hook <harness> [event]`: forwards a harness hook payload (stdin) to the pane's agent state.
/// Never fails the caller: outside mpx, or on any error, it prints nothing.
pub async fn hook(harness: &str, event: Option<&str>, key: Option<&str>) -> Result<i32, String> {
    if harness == "print-config" {
        return print_config(event);
    }
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    if let Some(report) = agents::hook_report(harness, event, &input) {
        agents::send_hook_report(harness, report, key).await;
    }
    Ok(0)
}

fn print_config(event: Option<&str>) -> Result<i32, String> {
    match event {
        Some("claude") => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            println!("{}", claude_settings_json(&exe));
            Ok(0)
        }
        _ => Err("print-config supports: claude".to_string()),
    }
}
