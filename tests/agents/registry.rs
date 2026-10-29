use mpx::agents::{SessionStore, detect, find_harness, registry};
use mpx::config::{Config, HarnessConfig};

fn with_harness(entry: HarnessConfig) -> Config {
    Config {
        harness: vec![entry],
        ..Config::default()
    }
}

#[test]
fn builtins_are_claude_omp_codex_rho_antigravity_and_opencode() {
    let reg = registry(&Config::default());
    let names: Vec<&str> = reg.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(
        names,
        ["claude", "omp", "codex", "rho", "antigravity", "opencode"]
    );
}

#[test]
fn config_entry_replaces_builtin_and_keeps_its_session_store() {
    let cfg = with_harness(HarnessConfig {
        name: "omp".into(),
        detect: vec!["myomp".into()],
        command: vec!["myomp".into()],
        resume: vec!["myomp".into(), "-r".into(), "{id}".into()],
        session_id_args: Vec::new(),
    });
    let reg = registry(&cfg);
    let omp = find_harness(&reg, "omp").expect("omp present");
    assert_eq!(omp.command, ["myomp"]);
    assert_eq!(omp.store, Some(SessionStore::Omp));
    assert_eq!(reg.iter().filter(|h| h.name == "omp").count(), 1);
}

#[test]
fn config_entry_with_new_name_is_appended() {
    let cfg = with_harness(HarnessConfig {
        name: "gemini".into(),
        detect: vec!["gemini".into()],
        command: vec!["gemini".into()],
        ..HarnessConfig::default()
    });
    let reg = registry(&cfg);
    assert_eq!(reg.len(), 7);
    assert_eq!(reg.last().map(|h| h.name.as_str()), Some("gemini"));
    assert_eq!(reg.last().and_then(|h| h.store), None);
}

#[test]
fn detect_matches_basename_case_insensitively() {
    let reg = registry(&Config::default());
    assert_eq!(
        detect(&reg, "claude").map(|h| h.name.as_str()),
        Some("claude")
    );
    assert_eq!(
        detect(&reg, "Claude").map(|h| h.name.as_str()),
        Some("claude")
    );
    assert_eq!(
        detect(&reg, "/usr/local/bin/codex").map(|h| h.name.as_str()),
        Some("codex")
    );
    assert_eq!(
        detect(&reg, "codex.exe").map(|h| h.name.as_str()),
        Some("codex")
    );
    assert_eq!(
        detect(&reg, "/usr/local/bin/rho").map(|h| h.name.as_str()),
        Some("rho")
    );
    assert_eq!(detect(&reg, "rho").map(|h| h.name.as_str()), Some("rho"));
    assert_eq!(
        detect(&reg, "antigravity").map(|h| h.name.as_str()),
        Some("antigravity")
    );
    assert_eq!(
        detect(&reg, "/usr/local/bin/agy").map(|h| h.name.as_str()),
        Some("antigravity")
    );
    assert_eq!(
        detect(&reg, "agy").map(|h| h.name.as_str()),
        Some("antigravity")
    );
    assert_eq!(
        detect(&reg, "/usr/local/bin/opencode").map(|h| h.name.as_str()),
        Some("opencode")
    );
    assert_eq!(
        detect(&reg, "opencode").map(|h| h.name.as_str()),
        Some("opencode")
    );
}

#[test]
fn detect_ignores_unrelated_processes() {
    let reg = registry(&Config::default());
    assert!(detect(&reg, "zsh").is_none());
    assert!(detect(&reg, "claude-helper").is_none());
    assert!(detect(&reg, "").is_none());
}

#[test]
fn detect_uses_aliases_from_config() {
    let cfg = with_harness(HarnessConfig {
        name: "claude".into(),
        detect: vec!["claude".into(), "2.1.308".into()],
        command: vec!["claude".into()],
        resume: vec!["claude".into(), "--resume".into(), "{id}".into()],
        session_id_args: vec!["--session-id".into(), "{id}".into()],
    });
    let reg = registry(&cfg);
    assert_eq!(
        detect(&reg, "2.1.308").map(|h| h.name.as_str()),
        Some("claude")
    );
}

#[test]
fn find_harness_matches_by_name_or_detect_alias() {
    let reg = registry(&Config::default());
    assert_eq!(
        find_harness(&reg, "antigravity").map(|h| h.name.as_str()),
        Some("antigravity")
    );
    assert_eq!(
        find_harness(&reg, "agy").map(|h| h.name.as_str()),
        Some("antigravity")
    );
    assert_eq!(
        find_harness(&reg, "opencode").map(|h| h.name.as_str()),
        Some("opencode")
    );
}
