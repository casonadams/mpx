use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::hooks::{claude_settings_json, omp_hook_file};
use super::{Harness, LaunchOptions, LaunchSpec, SessionStore};
use crate::config::{Config, HarnessConfig};

pub struct LaunchEnv<'a> {
    /// Absolute path of the running mpx binary, used for hook commands.
    pub exe: &'a Path,
    /// Extension file loaded by omp (`--hook=<file>`), when available.
    pub omp_hook: Option<&'a Path>,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn builtin() -> Vec<Harness> {
    vec![
        Harness {
            name: "claude".into(),
            detect: strings(&["claude"]),
            command: strings(&["claude"]),
            resume: strings(&["claude", "--resume", "{id}"]),
            session_id_args: strings(&["--session-id", "{id}"]),
            store: Some(SessionStore::Claude),
        },
        Harness {
            name: "omp".into(),
            detect: strings(&["omp"]),
            command: strings(&["omp"]),
            resume: strings(&["omp", "--resume", "{id}"]),
            session_id_args: Vec::new(),
            store: Some(SessionStore::Omp),
        },
        Harness {
            name: "codex".into(),
            detect: strings(&["codex"]),
            command: strings(&["codex"]),
            resume: strings(&["codex", "resume", "{id}"]),
            session_id_args: Vec::new(),
            store: Some(SessionStore::Codex),
        },
    ]
}

fn from_config(hc: &HarnessConfig) -> Harness {
    Harness {
        name: hc.name.clone(),
        detect: hc.detect.clone(),
        command: hc.command.clone(),
        resume: hc.resume.clone(),
        session_id_args: hc.session_id_args.clone(),
        store: None,
    }
}

/// Built-in harnesses overlaid with `[[harness]]` entries from config (same name replaces).
/// A replacement keeps the built-in session store, which config cannot describe.
pub fn registry(cfg: &Config) -> Vec<Harness> {
    let mut list = builtin();
    for hc in &cfg.harness {
        let mut h = from_config(hc);
        match list.iter_mut().find(|x| x.name == h.name) {
            Some(slot) => {
                h.store = slot.store;
                *slot = h;
            }
            None => list.push(h),
        }
    }
    list
}

/// Process name without directory or `.exe` suffix.
fn basename(process: &str) -> &str {
    let name = process.rsplit(['/', '\\']).next().unwrap_or(process);
    match name.len().checked_sub(4).and_then(|i| name.get(i..)) {
        Some(ext) if ext.eq_ignore_ascii_case(".exe") => &name[..name.len() - 4],
        _ => name,
    }
}

/// Harness whose `detect` list contains the process name (basename, case-insensitive).
pub fn detect<'a>(registry: &'a [Harness], process: &str) -> Option<&'a Harness> {
    let base = basename(process);
    registry
        .iter()
        .find(|h| h.detect.iter().any(|d| d.eq_ignore_ascii_case(base)))
}

fn fill(template: &[String], id: &str) -> Vec<String> {
    template.iter().map(|s| s.replace("{id}", id)).collect()
}

/// argv for a resume (when a template exists) or a fresh session, plus the session id it carries.
fn base_argv(h: &Harness, resume: Option<&str>) -> (Vec<String>, Option<String>) {
    if let Some(id) = resume.filter(|_| !h.resume.is_empty()) {
        return (fill(&h.resume, id), Some(id.to_string()));
    }
    let mut argv = h.command.clone();
    if h.session_id_args.is_empty() {
        return (argv, None);
    }
    let id = Uuid::new_v4().to_string();
    argv.extend(fill(&h.session_id_args, &id));
    (argv, Some(id))
}

fn harness_args(h: &Harness, env: &LaunchEnv, opts: LaunchOptions) -> Vec<String> {
    match h.name.as_str() {
        "claude" => {
            let mut args = vec!["--settings".to_string(), claude_settings_json(env.exe)];
            if opts.teams {
                args.extend(["--teammate-mode".to_string(), "tmux".to_string()]);
            }
            args
        }
        "omp" => env
            .omp_hook
            .map(|f| vec![format!("--hook={}", f.display())])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Launch spec built from the running binary and, for omp, the extension file written to the runtime dir.
pub fn launch(h: &Harness, resume: Option<&str>, opts: LaunchOptions) -> LaunchSpec {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mpx"));
    let hook = if h.name == "omp" {
        omp_hook_file()
    } else {
        None
    };
    launch_with(
        h,
        resume,
        opts,
        &LaunchEnv {
            exe: &exe,
            omp_hook: hook.as_deref(),
        },
    )
}

/// Pure launch spec construction; `launch` supplies the environment.
pub fn launch_with(
    h: &Harness,
    resume: Option<&str>,
    opts: LaunchOptions,
    env: &LaunchEnv,
) -> LaunchSpec {
    let (mut argv, session_id) = base_argv(h, resume);
    argv.extend(harness_args(h, env, opts));
    let mut spawn_env = vec![("MPX_AGENT_HARNESS".to_string(), h.name.clone())];
    if opts.teams {
        spawn_env.push((
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".to_string(),
            "1".to_string(),
        ));
    }
    LaunchSpec {
        name: h.name.clone(),
        argv,
        env: spawn_env,
        tmux_compat: opts.teams,
        session_id,
    }
}
