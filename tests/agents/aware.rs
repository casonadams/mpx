use std::path::{Path, PathBuf};

use mpx::agents::LiveAgent;
use mpx::agents::aware::{
    Me, Moment, Seen, claude_moment, context_json, peers, peers_json, repo_root, roster_text,
};
use mpx::protocol::{AgentFocus, AgentInfo, AgentState, EDIT_WINDOW_MS, FileEdit, NodeState};
use serde_json::{Value, json};

const NOW: u64 = 100 * 60_000;

/// A temp dir holding a repository `repo` (with `.git` and `src/`) and a plain dir `other`.
fn tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("git dir");
    std::fs::create_dir_all(repo.join("src")).expect("src dir");
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).expect("other dir");
    (dir, repo, other)
}

fn agent(session: &str, name: &str, cwd: &Path, task: Option<&str>, edits: &[(&str, u64)]) -> LiveAgent {
    let mut node = mpx::protocol::NodeInfo::new(name.to_string(), true, "claude".to_string());
    node.cwd = Some(cwd.to_string_lossy().into_owned());
    node.agent = Some(AgentInfo {
        harness: "claude".to_string(),
        state: AgentState::Working,
        session_id: None,
        detail: None,
        since_ms: 0,
        focus: Box::new(AgentFocus {
            task: task.map(str::to_string),
            reply: None,
            edits: edits
                .iter()
                .map(|(p, at)| FileEdit {
                    path: p.to_string(),
                    at_ms: *at,
                })
                .collect(),
        }),
    });
    LiveAgent {
        session: session.to_string(),
        endpoint: format!("/sock/{session}"),
        node,
    }
}

fn me(cwd: &Path) -> Me {
    Me {
        endpoint: "/sock/main".to_string(),
        pane: "claude-a".to_string(),
        cwd: cwd.to_path_buf(),
    }
}

fn refs(agents: &[LiveAgent]) -> Vec<String> {
    agents
        .iter()
        .map(|a| format!("{}:{}", a.session, a.node.name))
        .collect()
}

#[test]
fn repo_root_is_the_nearest_ancestor_with_git_or_the_dir_itself() {
    let (_dir, repo, other) = tree();
    let real = |p: &Path| p.canonicalize().expect("canonical");
    assert_eq!(repo_root(&repo.join("src")), real(&repo));
    assert_eq!(repo_root(&repo), real(&repo));
    assert_eq!(repo_root(&other), real(&other));
}

#[test]
fn peers_are_other_running_agents_in_the_same_repository() {
    let (_dir, repo, other) = tree();
    let mut exited = agent("main", "gone", &repo, None, &[]);
    exited.node.state = NodeState::Exited;
    let live = vec![
        agent("main", "claude-a", &repo, None, &[]),
        agent("main", "claude-b", &repo.join("src"), None, &[]),
        agent("billing", "api", &repo, None, &[]),
        agent("main", "elsewhere", &other, None, &[]),
        exited,
    ];
    let found = peers(live, &me(&repo.join("src")));
    assert_eq!(refs(&found), vec!["main:claude-b", "billing:api"]);
}

#[test]
fn roster_names_each_peer_with_its_task_and_how_to_reach_it() {
    let (_dir, repo, _) = tree();
    let text = roster_text(&[agent("billing", "api", &repo, Some("migrate billing to v2"), &[])]);
    assert!(text.contains("- billing:api (claude, working); task: migrate billing to v2"), "{text}");
    assert!(text.contains("mpx_peers") && text.contains("mpx_agent_prompt"), "{text}");
}

#[test]
fn roster_is_sent_only_when_the_set_of_peers_changes() {
    let (_dir, repo, _) = tree();
    let api = agent("billing", "api", &repo, None, &[]);
    let mut seen = Seen::default();
    assert_eq!(seen.roster_update(&[]), None, "nobody to mention yet");
    assert!(seen.roster_update(std::slice::from_ref(&api)).is_some());
    assert_eq!(seen.roster_update(std::slice::from_ref(&api)), None);
    let left = seen.roster_update(&[]).expect("departure is news");
    assert!(left.contains("no other agents"), "{left}");
    assert_eq!(seen.roster_update(&[]), None);
}

#[test]
fn file_notice_names_recent_editors_once_each() {
    let (_dir, repo, _) = tree();
    let path = "/repo/src/auth.rs";
    let api = agent("billing", "api", &repo, Some("rotate tokens"), &[(path, NOW - 2 * 60_000)]);
    let mut seen = Seen::default();
    let text = seen
        .file_update(path, std::slice::from_ref(&api), NOW)
        .expect("notice");
    assert!(
        text.contains("/repo/src/auth.rs was edited 2 minutes ago by agent billing:api while working on: rotate tokens"),
        "{text}"
    );
    assert_eq!(seen.file_update(path, std::slice::from_ref(&api), NOW), None);

    let web = agent("web", "ui", &repo, None, &[(path, NOW)]);
    let text = seen.file_update(path, &[api, web], NOW).expect("new editor");
    assert!(text.contains("web:ui") && !text.contains("billing:api"), "{text}");
}

#[test]
fn file_notice_ignores_edits_outside_the_window_and_other_files() {
    let (_dir, repo, _) = tree();
    let old = agent("billing", "api", &repo, None, &[("/a.rs", NOW - EDIT_WINDOW_MS)]);
    let mut seen = Seen::default();
    assert_eq!(seen.file_update("/a.rs", std::slice::from_ref(&old), NOW), None);
    assert_eq!(seen.file_update("/b.rs", &[old], NOW), None);
}

#[test]
fn seen_survives_a_save_and_load_and_forget_removes_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let seen = Seen {
        peers: vec!["billing:api".to_string()],
        noticed: ["/a.rs|billing:api".to_string()].into(),
    };
    seen.save(dir.path(), "sess/1");
    assert_eq!(Seen::load(dir.path(), "sess/1"), seen);
    Seen::forget(dir.path(), "sess/1");
    assert_eq!(Seen::load(dir.path(), "sess/1"), Seen::default());
}

#[test]
fn claude_moments_are_prompts_and_file_tools() {
    let event = |name: &str, extra: Value| {
        let mut v = json!({ "hook_event_name": name });
        if let (Some(obj), Some(more)) = (v.as_object_mut(), extra.as_object()) {
            obj.extend(more.clone());
        }
        v
    };
    assert_eq!(claude_moment(&event("SessionStart", json!({}))), Some(Moment::Roster));
    assert_eq!(claude_moment(&event("UserPromptSubmit", json!({}))), Some(Moment::Roster));
    assert_eq!(
        claude_moment(&event("PostToolUse", json!({ "tool_input": { "file_path": "/a.rs" } }))),
        Some(Moment::File("/a.rs".to_string()))
    );
    assert_eq!(claude_moment(&event("Stop", json!({}))), None);
}

#[test]
fn context_json_wraps_text_for_the_event() {
    let v: Value = serde_json::from_str(&context_json("PostToolUse", "hi")).expect("json");
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert_eq!(v["hookSpecificOutput"]["additionalContext"], "hi");
}

#[test]
fn peers_json_lists_focus_and_recent_edits_newest_first() {
    let (_dir, repo, other) = tree();
    let live = vec![
        agent(
            "billing",
            "api",
            &repo,
            Some("migrate"),
            &[("/old.rs", 0), ("/a.rs", NOW - 60_000), ("/b.rs", NOW)],
        ),
        agent("main", "elsewhere", &other, None, &[]),
    ];
    let v = peers_json(live.clone(), &me(&repo), false, NOW);
    assert_eq!(v["repository"], json!(repo.canonicalize().expect("canonical")));
    let rows = v["agents"].as_array().expect("agents");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["agent"], "billing:api");
    assert_eq!(rows[0]["task"], "migrate");
    let edits: Vec<&str> = rows[0]["recent_edits"]
        .as_array()
        .expect("edits")
        .iter()
        .map(|e| e["path"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(edits, vec!["/b.rs", "/a.rs"]);
    assert_eq!(peers_json(live, &me(&repo), true, NOW)["agents"].as_array().map(Vec::len), Some(2));
}
