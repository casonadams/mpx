use std::path::Path;

use mpx::agents::{
    LaunchEnv, LaunchOptions, claude_settings_json, find_harness, launch, launch_with,
    omp_extension_source, registry,
};
use mpx::config::{Config, HarnessConfig};

const EXE: &str = "/opt/mpx/bin/mpx";

fn env_none() -> LaunchEnv<'static> {
    LaunchEnv {
        exe: Path::new(EXE),
        omp_hook: None,
        omp_plugin: None,
    }
}

fn harness(name: &str) -> mpx::agents::Harness {
    let reg = registry(&Config::default());
    find_harness(&reg, name).expect("builtin harness").clone()
}

fn value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

fn env_value<'a>(spec: &'a mpx::agents::LaunchSpec, key: &str) -> Option<&'a str> {
    spec.env
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn fresh_claude_gets_a_uuid_session_id_in_argv() {
    let spec = launch_with(
        &harness("claude"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(spec.argv[0], "claude");
    let id = value_after(&spec.argv, "--session-id").expect("--session-id present");
    let parsed = uuid::Uuid::parse_str(id).expect("uuid");
    assert_eq!(parsed.get_version_num(), 4);
    assert_eq!(spec.session_id.as_deref(), Some(id));
}

#[test]
fn resume_uses_the_resume_template_and_carries_the_id() {
    let spec = launch_with(
        &harness("claude"),
        Some("abc-123"),
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(&spec.argv[..3], ["claude", "--resume", "abc-123"]);
    assert!(value_after(&spec.argv, "--session-id").is_none());
    assert_eq!(spec.session_id.as_deref(), Some("abc-123"));
}

#[test]
fn codex_has_no_preassigned_session_id() {
    let spec = launch_with(
        &harness("codex"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(spec.argv, ["codex"]);
    assert_eq!(spec.session_id, None);
}

#[test]
fn codex_resume_uses_subcommand_form() {
    let spec = launch_with(
        &harness("codex"),
        Some("s1"),
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(spec.argv, ["codex", "resume", "s1"]);
}

#[test]
fn rho_has_no_preassigned_session_id() {
    let spec = launch_with(&harness("rho"), None, LaunchOptions::default(), &env_none());
    assert_eq!(spec.argv, ["rho"]);
    assert_eq!(spec.session_id, None);
}

#[test]
fn rho_resume_uses_resume_flag() {
    let spec = launch_with(
        &harness("rho"),
        Some("20261028_065219_78ecbb1d"),
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(spec.argv, ["rho", "--resume", "20261028_065219_78ecbb1d"]);
}

#[test]
fn antigravity_and_opencode_launch_and_resume() {
    let fresh_agy = launch_with(
        &harness("antigravity"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(fresh_agy.argv, ["agy"]);
    assert_eq!(fresh_agy.session_id, None);

    let res_agy = launch_with(
        &harness("antigravity"),
        Some("c1"),
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(res_agy.argv, ["agy", "--conversation", "c1"]);

    let fresh_opencode = launch_with(
        &harness("opencode"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(fresh_opencode.argv, ["opencode"]);
    assert_eq!(fresh_opencode.session_id, None);

    let res_opencode = launch_with(
        &harness("opencode"),
        Some("s1"),
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(res_opencode.argv, ["opencode", "--session", "s1"]);
}

#[test]
fn every_launch_sets_the_harness_env() {
    let spec = launch_with(&harness("omp"), None, LaunchOptions::default(), &env_none());
    assert_eq!(env_value(&spec, "MPX_AGENT_HARNESS"), Some("omp"));
}

#[test]
fn claude_settings_register_hooks_for_all_mpx_events() {
    let spec = launch_with(
        &harness("claude"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    let raw = value_after(&spec.argv, "--settings").expect("--settings present");
    let settings: serde_json::Value = serde_json::from_str(raw).expect("settings is JSON");
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "Notification",
        "Stop",
        "SessionEnd",
    ] {
        let entry = &settings["hooks"][event][0];
        assert!(entry.get("matcher").is_none(), "{event} has no matcher");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(entry["hooks"][0]["command"], format!("'{EXE}' hook claude"));
    }
}

#[test]
fn claude_settings_quote_paths_with_spaces_and_quotes() {
    let env = LaunchEnv {
        exe: Path::new("/opt/my apps/it's/mpx"),
        omp_hook: None,
        omp_plugin: None,
    };
    let spec = launch_with(&harness("claude"), None, LaunchOptions::default(), &env);
    let raw = value_after(&spec.argv, "--settings").expect("--settings present");
    let settings: serde_json::Value = serde_json::from_str(raw).expect("settings is JSON");
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0]["command"],
        "'/opt/my apps/it'\\''s/mpx' hook claude"
    );
}

#[test]
fn teams_set_env_tmux_compat_and_teammate_mode() {
    let opts = LaunchOptions {
        teams: true,
        ..LaunchOptions::default()
    };
    let spec = launch_with(&harness("claude"), None, opts, &env_none());
    assert_eq!(
        env_value(&spec, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some("1")
    );
    assert!(spec.tmux_compat);
    assert_eq!(value_after(&spec.argv, "--teammate-mode"), Some("tmux"));
}

#[test]
fn without_teams_no_teammate_flag_or_env() {
    let spec = launch_with(
        &harness("claude"),
        None,
        LaunchOptions::default(),
        &env_none(),
    );
    assert_eq!(
        env_value(&spec, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        None
    );
    assert!(!spec.tmux_compat);
    assert!(value_after(&spec.argv, "--teammate-mode").is_none());
}

#[test]
fn omp_gets_the_extension_file_when_available() {
    let hook = Path::new("/tmp/mpx/hooks/omp-agent.js");
    let env = LaunchEnv {
        exe: Path::new(EXE),
        omp_hook: Some(hook),
        omp_plugin: None,
    };
    let spec = launch_with(&harness("omp"), None, LaunchOptions::default(), &env);
    assert!(
        spec.argv
            .contains(&"--hook=/tmp/mpx/hooks/omp-agent.js".to_string())
    );
}

#[test]
fn custom_harness_substitutes_id_in_session_args() {
    let cfg = Config {
        harness: vec![HarnessConfig {
            name: "tool".into(),
            detect: vec!["tool".into()],
            command: vec!["tool".into(), "run".into()],
            resume: vec!["tool".into(), "--continue".into(), "{id}".into()],
            session_id_args: vec!["--id".into(), "{id}".into()],
        }],
        ..Config::default()
    };
    let reg = registry(&cfg);
    let tool = find_harness(&reg, "tool").expect("custom harness");
    let spec = launch_with(tool, None, LaunchOptions::default(), &env_none());
    let id = value_after(&spec.argv, "--id").expect("--id present");
    assert_eq!(spec.argv, ["tool", "run", "--id", id]);
    assert!(uuid::Uuid::parse_str(id).is_ok());
}

fn exe_path() -> std::path::PathBuf {
    std::env::current_exe().expect("current exe")
}

#[test]
fn omp_launch_writes_the_extension_for_the_current_exe() {
    let runtime = tempfile::tempdir().expect("temp dir");
    let spec = launch(
        &harness("omp"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    let hook = runtime.path().join("hooks").join("omp-agent.js");
    assert!(spec.argv.contains(&format!("--hook={}", hook.display())));
    let source = std::fs::read_to_string(&hook).expect("extension file");
    assert_eq!(source, omp_extension_source(&exe_path()));
}

#[test]
fn relaunch_keeps_an_unchanged_extension_file() {
    let runtime = tempfile::tempdir().expect("temp dir");
    let hook = runtime.path().join("hooks").join("omp-agent.js");
    launch(
        &harness("omp"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    let mut perms = std::fs::metadata(&hook)
        .expect("hook metadata")
        .permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&hook, perms).expect("make read-only");
    let spec = launch(
        &harness("omp"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    assert!(spec.argv.contains(&format!("--hook={}", hook.display())));
    let source = std::fs::read_to_string(&hook).expect("extension file");
    assert_eq!(source, omp_extension_source(&exe_path()));
}

#[test]
fn claude_launch_registers_hooks_and_a_fresh_session_id() {
    let runtime = tempfile::tempdir().expect("temp dir");
    let spec = launch(
        &harness("claude"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    let id = spec.session_id.as_deref().expect("fresh session id");
    assert!(uuid::Uuid::parse_str(id).is_ok());
    assert_eq!(value_after(&spec.argv, "--session-id"), Some(id));
    let settings = claude_settings_json(&exe_path());
    assert_eq!(
        value_after(&spec.argv, "--settings"),
        Some(settings.as_str())
    );
}

#[test]
fn claude_launch_resume_applies_the_template_and_keeps_the_id() {
    let runtime = tempfile::tempdir().expect("temp dir");
    let spec = launch(
        &harness("claude"),
        Some("sess-42"),
        LaunchOptions::default(),
        runtime.path(),
    );
    assert_eq!(spec.argv[..3].to_vec(), ["claude", "--resume", "sess-42"]);
    assert_eq!(spec.session_id.as_deref(), Some("sess-42"));
}

#[test]
fn launches_name_their_harness_in_the_environment() {
    let runtime = tempfile::tempdir().expect("temp dir");
    let claude = launch(
        &harness("claude"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    let omp = launch(
        &harness("omp"),
        None,
        LaunchOptions::default(),
        runtime.path(),
    );
    assert_eq!(env_value(&claude, "MPX_AGENT_HARNESS"), Some("claude"));
    assert_eq!(env_value(&omp, "MPX_AGENT_HARNESS"), Some("omp"));
}

#[test]
fn claude_mcp_option_adds_the_mpx_server_ahead_of_the_settings() {
    let opts = LaunchOptions {
        mcp: true,
        ..LaunchOptions::default()
    };
    let spec = launch_with(&harness("claude"), None, opts, &env_none());
    let at = spec
        .argv
        .iter()
        .position(|a| a == "--mcp-config")
        .expect("--mcp-config passed");
    assert_eq!(spec.argv[at + 2], "--settings", "the next option ends the value list");
    let config: serde_json::Value = serde_json::from_str(&spec.argv[at + 1]).expect("json");
    assert_eq!(config["mcpServers"]["mpx"]["command"], EXE);
    assert_eq!(config["mcpServers"]["mpx"]["args"], serde_json::json!(["mcp"]));

    let plain = launch_with(&harness("claude"), None, LaunchOptions::default(), &env_none());
    assert!(!plain.argv.iter().any(|a| a == "--mcp-config"));
}

#[test]
fn claude_settings_watch_file_tools_after_use() {
    let settings: serde_json::Value =
        serde_json::from_str(&claude_settings_json(Path::new(EXE))).expect("json");
    let post = &settings["hooks"]["PostToolUse"][0];
    let matcher = post["matcher"].as_str().expect("matcher");
    for tool in ["Read", "Edit", "Write"] {
        assert!(matcher.split('|').any(|t| t == tool), "{matcher}");
    }
    assert!(settings["hooks"]["Stop"][0].get("matcher").is_none());
}

#[test]
fn omp_gets_mpx_mcp_tools_only_when_mcp_is_on() {
    let env = LaunchEnv {
        exe: Path::new(EXE),
        omp_hook: None,
        omp_plugin: Some(Path::new("/tmp/mpx/hooks/omp-plugin")),
    };
    let with = LaunchOptions {
        mcp: true,
        ..LaunchOptions::default()
    };
    let spec = launch_with(&harness("omp"), None, with, &env);
    assert!(spec.argv.contains(&"--plugin-dir=/tmp/mpx/hooks/omp-plugin".to_string()));
    let without = LaunchOptions {
        mcp: false,
        ..LaunchOptions::default()
    };
    let spec = launch_with(&harness("omp"), None, without, &env);
    assert!(!spec.argv.iter().any(|a| a.starts_with("--plugin-dir")));
}
