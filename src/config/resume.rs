use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ResumeRule {
    pub name: String,
    /// Foreground or command process names that identify this tool.
    pub detect: Vec<String>,
    /// Command line to resume a session; `{id}` is replaced by the session id.
    pub resume: Vec<String>,
    /// Command line fallback when no session id is known (e.g. `["claude", "--continue"]`).
    pub fallback: Vec<String>,
    /// Optional environment variable that carries the session id (read from the daemon's environment).
    pub env_id: Option<String>,
    /// Where to find the session id of the tool running in the pane, tried before `env_id`.
    pub id_source: Option<IdSource>,
}

/// State files that map a live process to its session.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdSource {
    /// Claude Code: `~/.claude/sessions/<pid>.json`.
    Claude,
    /// oh-my-pi: `~/.omp/agent/terminal-sessions/<tty>`.
    Omp,
}

pub fn default_resume_rules() -> Vec<ResumeRule> {
    vec![
        ResumeRule {
            env_id: Some("CLAUDE_SESSION_ID".into()),
            id_source: Some(IdSource::Claude),
            ..rule(
                "claude",
                &["claude"],
                &["claude", "--resume", "{id}"],
                &["claude", "--continue"],
            )
        },
        ResumeRule {
            env_id: Some("OMP_SESSION_ID".into()),
            id_source: Some(IdSource::Omp),
            ..rule(
                "omp",
                &["omp"],
                &["omp", "--restore", "{id}"],
                &["omp", "--continue"],
            )
        },
        rule(
            "codex",
            &["codex"],
            &["codex", "resume", "{id}"],
            &["codex", "resume", "--last"],
        ),
        rule(
            "rho",
            &["rho"],
            &["rho", "--resume", "{id}"],
            &["rho", "--resume"],
        ),
        rule(
            "antigravity",
            &["antigravity", "agy"],
            &["agy", "--conversation", "{id}"],
            &["agy", "--continue"],
        ),
        rule(
            "opencode",
            &["opencode"],
            &["opencode", "--session", "{id}"],
            &["opencode"],
        ),
        rule(
            "nvim",
            &["nvim"],
            &[],
            &["nvim", "-c", "silent! source Session.vim"],
        ),
    ]
}

/// Rule with no env var or id source; callers override those via struct update.
fn rule(name: &str, detect: &[&str], resume: &[&str], fallback: &[&str]) -> ResumeRule {
    ResumeRule {
        name: name.into(),
        detect: words(detect),
        resume: words(resume),
        fallback: words(fallback),
        env_id: None,
        id_source: None,
    }
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).into()).collect()
}
