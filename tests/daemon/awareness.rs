//! Agent awareness across sessions: hook reports land in each daemon, and another agent's hooks
//! and MCP tools read them back from every session sharing the socket directory.
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::support::{connect, logs_until, pty, serve_session};
use mpx::agents::aware::{HookEvent, HookSite, Me, context_json, context_text};
use mpx::client::{AgentReportArgs, Client};
use mpx::config::Config;
use mpx::mcp::McpServer;
use mpx::node::now_ms;
use mpx::protocol::{AgentState, SpawnOpts};

/// `alpha:a` and `beta:b` are agents in one repository; `a` is working on a task and edited
/// `src/auth.rs`.
struct Pair {
    dir: tempfile::TempDir,
    repo: PathBuf,
    beta: String,
    alpha_client: Client,
}

impl Pair {
    fn sockets(&self) -> PathBuf {
        self.dir.path().join("run")
    }

    fn auth(&self) -> String {
        self.repo.join("src/auth.rs").to_string_lossy().into_owned()
    }

    fn payload(&self, event: &str, extra: Value) -> String {
        let mut v = json!({
            "hook_event_name": event,
            "session_id": "sess-b",
            "cwd": self.repo.join("src"),
        });
        if let (Some(obj), Some(more)) = (v.as_object_mut(), extra.as_object()) {
            obj.extend(more.clone());
        }
        v.to_string()
    }

    async fn context(&self, event: &str, extra: Value) -> Option<String> {
        let v: Value = serde_json::from_str(&self.payload(event, extra)).ok()?;
        let hooked = HookEvent::claude(&v)?;
        let sockets = self.sockets();
        let site = HookSite {
            endpoint: &self.beta,
            pane: "b",
            socket_dir: &sockets,
        };
        let text = context_text(&hooked, &site, now_ms()).await?;
        Some(context_json(&hooked.name, &text))
    }
}

fn agent_in(name: &str, cwd: &Path) -> SpawnOpts {
    SpawnOpts {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        ..pty(name, "cat", &[])
    }
}

async fn report(client: &mut Client, pane: &str, args: AgentReportArgs) {
    let args = AgentReportArgs {
        harness: Some("claude".to_string()),
        ..args
    };
    client.agent_report(pane, args).await.expect("report");
}

async fn pair() -> Pair {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("git");
    std::fs::create_dir_all(repo.join("src")).expect("src");
    let sockets = dir.path().join("run");
    std::fs::create_dir_all(&sockets).expect("sockets");
    let alpha = serve_session(&sockets, "alpha", &dir.path().join("sa"), Config::default()).await;
    let beta = serve_session(&sockets, "beta", &dir.path().join("sb"), Config::default()).await;

    let mut a = connect(&alpha).await;
    a.spawn(agent_in("a", &repo)).await.expect("spawn a");
    let auth = repo.join("src/auth.rs").to_string_lossy().into_owned();
    report(
        &mut a,
        "a",
        AgentReportArgs {
            state: Some(AgentState::Working),
            task: Some("rotate auth tokens".to_string()),
            ..AgentReportArgs::default()
        },
    )
    .await;
    report(
        &mut a,
        "a",
        AgentReportArgs {
            edited: Some(auth),
            ..AgentReportArgs::default()
        },
    )
    .await;

    let mut b = connect(&beta).await;
    b.spawn(agent_in("b", &repo.join("src"))).await.expect("spawn b");
    report(
        &mut b,
        "b",
        AgentReportArgs {
            state: Some(AgentState::Idle),
            ..AgentReportArgs::default()
        },
    )
    .await;
    Pair {
        dir,
        repo,
        beta,
        alpha_client: a,
    }
}

fn additional(context: &str) -> String {
    let v: Value = serde_json::from_str(context).expect("hook json");
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context text")
        .to_string()
}

#[tokio::test]
async fn daemon_keeps_an_agents_task_and_edits_for_others_to_list() {
    let mut p = pair().await;
    let nodes = p.alpha_client.list().await.expect("list");
    let focus = &nodes[0].agent.as_ref().expect("agent").focus;
    assert_eq!(focus.task.as_deref(), Some("rotate auth tokens"));
    assert_eq!(focus.edits.len(), 1);
    assert_eq!(focus.edits[0].path, p.auth());
}

#[tokio::test]
async fn hooks_tell_an_agent_about_peers_once_and_about_their_files_once() {
    let p = pair().await;
    let roster = additional(&p.context("SessionStart", json!({})).await.expect("roster"));
    assert!(roster.contains("alpha:a") && roster.contains("rotate auth tokens"), "{roster}");
    assert!(!roster.contains("beta:b"), "an agent is not its own peer: {roster}");
    assert_eq!(p.context("UserPromptSubmit", json!({})).await, None);

    let read = json!({ "tool_name": "Read", "tool_input": { "file_path": p.auth() } });
    let notice = additional(&p.context("PostToolUse", read.clone()).await.expect("notice"));
    assert!(notice.contains(&p.auth()) && notice.contains("alpha:a"), "{notice}");
    assert_eq!(p.context("PostToolUse", read).await, None);

    let other = json!({ "tool_name": "Read", "tool_input": { "file_path": "/elsewhere.rs" } });
    assert_eq!(p.context("PostToolUse", other).await, None);

    assert_eq!(p.context("SessionEnd", json!({})).await, None);
    assert!(!p.sockets().join("aware/sess-b.json").exists());
}

async fn mcp_call(p: &Pair, tool: &str, args: Value) -> Value {
    let me = Me {
        endpoint: p.beta.clone(),
        pane: "b".to_string(),
        cwd: p.repo.join("src"),
    };
    let server = McpServer::new(p.beta.clone(), "beta".into(), p.sockets(), Config::default())
        .with_caller(Some(me));
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": args }
    });
    let mut out = Vec::new();
    server
        .serve(format!("{request}\n").as_bytes(), &mut out)
        .await
        .expect("serve");
    let reply: Value = serde_json::from_slice(&out).expect("reply");
    let text = reply["result"]["content"][0]["text"].as_str().expect("text");
    serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
}

#[tokio::test]
async fn mpx_peers_lists_what_other_agents_in_the_repository_are_doing() {
    let p = pair().await;
    let v = mcp_call(&p, "mpx_peers", json!({})).await;
    let agents = v["agents"].as_array().expect("agents");
    assert_eq!(agents.len(), 1, "{v}");
    assert_eq!(agents[0]["agent"], "alpha:a");
    assert_eq!(agents[0]["task"], "rotate auth tokens");
    assert_eq!(agents[0]["recent_edits"][0]["path"], p.auth());
}

#[tokio::test]
async fn mpx_agent_prompt_reaches_an_agent_in_another_session() {
    let mut p = pair().await;
    let v = mcp_call(
        &p,
        "mpx_agent_prompt",
        json!({ "name": "alpha:a", "text": "hello-peer", "wait": false }),
    )
    .await;
    assert_eq!(v["submitted"], true, "{v}");
    logs_until(&mut p.alpha_client, "a", "hello-peer").await;
}
