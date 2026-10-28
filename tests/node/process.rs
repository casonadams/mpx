use mpx::node::process::{
    Identity, basename, detection_names, foreground_name, is_runtime, parse_cmdline,
    parse_procargs2,
};

fn id(comm: Option<&str>, argv: &[&str]) -> Identity {
    Identity {
        comm: comm.map(str::to_string),
        argv: argv.iter().map(|a| a.to_string()).collect(),
    }
}

#[test]
fn cmdline_splits_on_nul() {
    let raw = b"node\0/opt/tools/claude\0--resume\0";
    assert_eq!(
        parse_cmdline(raw),
        vec!["node", "/opt/tools/claude", "--resume"]
    );
}

#[test]
fn procargs2_skips_argc_exec_path_and_padding() {
    let mut blob = 2i32.to_ne_bytes().to_vec();
    blob.extend_from_slice(b"/usr/local/bin/node\0\0\0");
    blob.extend_from_slice(b"node\0/x/omp\0");
    assert_eq!(
        parse_procargs2(&blob),
        Some(vec!["node".to_string(), "/x/omp".to_string()])
    );
}

#[test]
fn procargs2_rejects_truncated_blobs() {
    assert_eq!(parse_procargs2(&[1, 0]), None);
}

#[test]
fn runtimes_are_recognised() {
    assert!(is_runtime("node"));
    assert!(is_runtime("bun"));
    assert!(is_runtime("deno"));
    assert!(is_runtime("python3.12"));
    assert!(!is_runtime("zsh"));
    assert!(!is_runtime("claude"));
}

#[test]
fn basename_strips_directories() {
    assert_eq!(basename("/usr/local/bin/claude"), "claude");
    assert_eq!(basename("claude"), "claude");
}

#[test]
fn version_string_comm_is_named_after_argv0() {
    let claude = id(Some("2.1.308"), &["claude", "--resume"]);
    assert_eq!(foreground_name(&claude).as_deref(), Some("claude"));
}

#[test]
fn version_string_comm_is_still_detectable_by_argv0() {
    let claude = id(Some("2.1.308"), &["/Users/x/.local/bin/claude"]);
    assert_eq!(detection_names(&claude), vec!["2.1.308", "claude"]);
}

#[test]
fn comm_is_kept_when_it_matches_argv0() {
    assert_eq!(
        foreground_name(&id(Some("vim"), &["vim", "x.rs"])).as_deref(),
        Some("vim")
    );
}

#[test]
fn login_shell_dash_is_ignored_when_comparing() {
    let shell = id(Some("zsh"), &["-zsh"]);
    assert_eq!(foreground_name(&shell).as_deref(), Some("zsh"));
    assert_eq!(detection_names(&shell), vec!["zsh"]);
}

#[test]
fn interpreter_names_the_script_it_runs() {
    let omp = id(Some("node"), &["node", "/opt/omp/bin/omp.js"]);
    assert_eq!(foreground_name(&omp).as_deref(), Some("omp.js"));
    assert_eq!(detection_names(&omp), vec!["node", "omp.js"]);
}

#[test]
fn interpreter_argv0_is_not_used_as_display_name() {
    let bun_run = id(Some("2.0.1"), &["bun", "/x/codex"]);
    assert_eq!(foreground_name(&bun_run).as_deref(), Some("2.0.1"));
    assert_eq!(detection_names(&bun_run), vec!["2.0.1", "bun", "codex"]);
}

#[test]
fn missing_comm_falls_back_to_argv() {
    assert_eq!(
        foreground_name(&id(None, &["claude"])).as_deref(),
        Some("claude")
    );
    assert_eq!(
        foreground_name(&id(None, &["node", "/x/tool"])).as_deref(),
        Some("tool")
    );
    assert_eq!(foreground_name(&id(None, &[])), None);
}

#[test]
fn empty_comm_is_ignored() {
    assert_eq!(
        foreground_name(&id(Some(""), &["claude"])).as_deref(),
        Some("claude")
    );
    assert_eq!(detection_names(&id(Some(""), &[])), Vec::<String>::new());
}

#[test]
fn detection_names_are_deduplicated_in_order() {
    let names = detection_names(&id(Some("claude"), &["claude", "claude"]));
    assert_eq!(names, vec!["claude"]);
}
