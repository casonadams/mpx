use mpx::daemon::plan::{self, PaneIdentity};

fn pane(name: &str, id: u32) -> PaneIdentity<'_> {
    PaneIdentity {
        endpoint: "/tmp/mpx.sock",
        session: "work",
        name,
        id,
    }
}

fn value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn identity_variables_are_always_present() {
    let env = plan::pane_env(&pane("api", 7), &[]);
    assert_eq!(value(&env, "MPX_ENDPOINT"), Some("/tmp/mpx.sock"));
    assert_eq!(value(&env, "MPX_SESSION"), Some("work"));
    assert_eq!(value(&env, "MPX_PANE"), Some("api"));
    assert_eq!(value(&env, "MPX_PANE_ID"), Some("%7"));
}

#[test]
fn spawn_env_is_appended_and_wins_on_conflict() {
    let extra = vec![
        ("GREETING".to_string(), "hi".to_string()),
        ("MPX_PANE".to_string(), "override".to_string()),
    ];
    let env = plan::pane_env(&pane("api", 0), &extra);
    assert_eq!(value(&env, "GREETING"), Some("hi"));
    let last_pane = env.iter().rev().find(|(k, _)| k == "MPX_PANE").unwrap();
    assert_eq!(last_pane.1, "override");
}

#[test]
fn spawn_name_prefers_request_and_derives_otherwise() {
    assert_eq!(plan::spawn_name("worker", "cat", 5), "worker");
    assert_eq!(plan::spawn_name("", "/usr/bin/cat", 5), "cat-5");
    assert_eq!(plan::spawn_name("", "claude", 0), "claude-0");
}

#[test]
fn names_must_be_nonempty_and_not_pane_refs() {
    assert!(plan::validate_name("ok").is_ok());
    assert!(plan::validate_name("").is_err());
    assert!(plan::validate_name("%3").is_err());
}

#[test]
fn default_program_is_the_default_command_through_the_shell() {
    assert_eq!(
        plan::default_argv(Some("/bin/zsh"), Some("/bin/bash"), None),
        ("/bin/zsh".to_string(), Vec::new())
    );
    assert_eq!(
        plan::default_argv(None, Some("/bin/bash"), Some("echo hi")),
        ("/bin/bash".to_string(), vec!["-c".to_string(), "echo hi".to_string()])
    );
    let (program, args) = plan::default_argv(None, None, None);
    let fallback = if cfg!(windows) { "powershell.exe" } else { "/bin/sh" };
    assert_eq!((program.as_str(), args.is_empty()), (fallback, true));
}

#[test]
fn cwd_anchor_follows_placement() {
    assert_eq!(
        plan::cwd_anchor(true, Some("a"), Some("b")),
        None,
        "plain spawn inherits nothing"
    );
    assert_eq!(
        plan::cwd_anchor(false, Some("a"), Some("b")),
        Some("a"),
        "split target wins"
    );
    assert_eq!(
        plan::cwd_anchor(false, None, Some("b")),
        Some("b"),
        "window or bare split uses focus"
    );
    assert_eq!(plan::cwd_anchor(false, None, None), None);
}
