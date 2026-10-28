use mpx::layout::Rect;
use mpx::protocol::Side;
use mpx::tmux_shim::{
    PaneView, endpoint_from_env, expand, is_noop, neighbor, pane_key, pane_vars, parse_invocation,
    session_of, session_vars, side_of, window_key,
};

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn parse(items: &[&str]) -> mpx::tmux_shim::Invocation {
    parse_invocation(&argv(items)).expect("valid tmux argv")
}

fn pane(id: u32, index: usize) -> PaneView {
    PaneView {
        session: "main".into(),
        id,
        index,
        pid: Some(4242),
        title: "build".into(),
        command: "claude".into(),
        path: "/work".into(),
        width: 80,
        height: 24,
        active: true,
        dead: false,
        window: 1,
        window_name: "dev".into(),
    }
}

#[test]
fn split_window_flags_values_and_trailing_command() {
    let inv = parse(&[
        "split-window",
        "-h",
        "-t",
        "%1",
        "-P",
        "-F",
        "#{pane_id}",
        "sh -c 'echo hi'",
    ]);
    assert_eq!(inv.command, "split-window");
    assert!(inv.has('h') && inv.has('P'));
    assert_eq!(inv.value('t'), Some("%1"));
    assert_eq!(inv.value('F'), Some("#{pane_id}"));
    assert_eq!(inv.operands, ["sh -c 'echo hi'"]);
    assert_eq!(inv.operand_text(), "sh -c 'echo hi'");
}

#[test]
fn attached_option_values_and_clustered_switches() {
    let inv = parse(&["list-panes", "-as", "-tmain:0"]);
    assert!(inv.has('a') && inv.has('s'));
    assert_eq!(inv.value('t'), Some("main:0"));
}

#[test]
fn double_dash_ends_options_so_operands_may_start_with_dash() {
    let inv = parse(&["send-keys", "-t", "%2", "-l", "--", "-x"]);
    assert!(inv.has('l'));
    assert_eq!(inv.operands, ["-x"]);
}

#[test]
fn option_parsing_stops_at_the_first_operand() {
    let inv = parse(&["send-keys", "-t", "%2", "Enter", "-l"]);
    assert!(!inv.has('l'));
    assert_eq!(inv.operands, ["Enter", "-l"]);
}

#[test]
fn command_aliases_resolve_to_canonical_names() {
    assert_eq!(parse(&["ls"]).command, "list-sessions");
    assert_eq!(parse(&["lsp"]).command, "list-panes");
    assert_eq!(parse(&["has", "-t", "main"]).command, "has-session");
    assert_eq!(mpx::tmux_shim::canonical("neww"), "new-window");
}

#[test]
fn unknown_and_incomplete_options_are_errors() {
    assert!(parse_invocation(&argv(&["list-panes", "-Z"])).is_err());
    assert!(parse_invocation(&argv(&["split-window", "-t"])).is_err());
}

#[test]
fn global_options_and_version() {
    assert_eq!(parse(&["-V"]).command, "-V");
    assert_eq!(parse(&["-L", "socket", "list-panes"]).command, "list-panes");
    assert!(parse_invocation(&[]).is_err());
}

#[test]
fn configuration_commands_are_accepted_noops() {
    for cmd in ["set-option", "set-window-option", "bind-key", "source-file"] {
        assert!(is_noop(cmd), "{cmd}");
    }
    assert!(!is_noop("split-window"));
    assert!(
        parse_invocation(&argv(&[
            "set-option",
            "-p",
            "-t",
            "%1",
            "remain-on-exit",
            "failed"
        ]))
        .is_ok()
    );
}

#[test]
fn pane_format_expands_known_variables() {
    let vars = pane_vars(&pane(3, 1));
    assert_eq!(expand("#{pane_id} #{pane_index}", &vars), "%3 1");
    assert_eq!(
        expand("#{pane_current_command}@#{pane_current_path}", &vars),
        "claude@/work"
    );
    assert_eq!(expand("#{pane_active}#{pane_dead}", &vars), "10");
    assert_eq!(expand("#{window_id}:#{window_name}", &vars), "@1:dev");
}

#[test]
fn unknown_variables_expand_to_nothing_and_unterminated_refs_stay_literal() {
    let vars = pane_vars(&pane(3, 1));
    assert_eq!(expand("[#{no_such_var}]", &vars), "[]");
    assert_eq!(expand("a #{pane_id", &vars), "a #{pane_id");
    assert_eq!(expand("plain text", &vars), "plain text");
}

#[test]
fn missing_pane_pid_expands_empty() {
    let mut p = pane(1, 0);
    p.pid = None;
    p.dead = true;
    let vars = pane_vars(&p);
    assert_eq!(expand("[#{pane_pid}]#{pane_dead}", &vars), "[]1");
}

#[test]
fn session_format_expands_counts() {
    let vars = session_vars("team", 2, 5);
    assert_eq!(
        expand("#{session_name}:#{session_windows}/#{session_panes}", &vars),
        "team:2/5"
    );
}

#[test]
fn endpoint_prefers_tmux_variable_over_mpx_endpoint() {
    assert_eq!(
        endpoint_from_env(Some("/tmp/mpx/main.sock,123,0"), Some("/other.sock")).as_deref(),
        Some("/tmp/mpx/main.sock")
    );
    assert_eq!(
        endpoint_from_env(None, Some("/other.sock")).as_deref(),
        Some("/other.sock")
    );
    assert_eq!(endpoint_from_env(Some(""), None), None);
    assert_eq!(endpoint_from_env(None, None), None);
}

#[test]
fn session_name_comes_from_the_endpoint_file_stem() {
    assert_eq!(session_of("/tmp/mpx/work.sock"), "work");
    assert_eq!(session_of("/tmp/mpx/mpx-team.sock"), "team");
}

#[test]
fn targets_reduce_to_pane_keys_and_window_indices() {
    assert_eq!(pane_key("main:0.%3"), "%3");
    assert_eq!(pane_key("%7"), "%7");
    assert_eq!(pane_key("main:work"), "work");
    assert_eq!(window_key("main:2"), Some(2));
    assert_eq!(window_key("@4"), Some(4));
    assert_eq!(window_key("%3"), None);
}

#[test]
fn side_flags_map_to_directions() {
    assert_eq!(side_of(&parse(&["select-pane", "-L"])), Some(Side::Left));
    assert_eq!(side_of(&parse(&["select-pane", "-D"])), Some(Side::Down));
    assert_eq!(side_of(&parse(&["select-pane", "-t", "%1"])), None);
}

fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn neighbor_finds_the_adjacent_overlapping_pane() {
    let layout = vec![
        ("left".to_string(), rect(0, 0, 40, 24), true),
        ("top-right".to_string(), rect(40, 0, 40, 12), false),
        ("bottom-right".to_string(), rect(40, 12, 40, 12), false),
    ];
    assert_eq!(
        neighbor(&layout, "left", Side::Right).as_deref(),
        Some("top-right")
    );
    assert_eq!(
        neighbor(&layout, "bottom-right", Side::Left).as_deref(),
        Some("left")
    );
    assert_eq!(
        neighbor(&layout, "top-right", Side::Down).as_deref(),
        Some("bottom-right")
    );
    assert_eq!(neighbor(&layout, "left", Side::Left), None);
    assert_eq!(neighbor(&layout, "missing", Side::Right), None);
}

#[test]
fn pane_view_takes_its_window_name_from_the_layout_tabs() {
    use mpx::layout::WindowTab;
    use mpx::protocol::NodeInfo;
    use mpx::tmux_shim::pane_view;
    let mut node = NodeInfo::new("build".into(), true, "sh".into());
    node.window = Some(1);
    let tabs = vec![
        WindowTab { index: 0, name: "editor".into(), is_active: true },
        WindowTab { index: 1, name: "logs".into(), is_active: false },
    ];
    let view = pane_view("dev", 0, &node, &[], &tabs);
    assert_eq!(expand("#{window_index}:#{window_name}", &pane_vars(&view)), "1:logs");
    assert_eq!(pane_view("dev", 0, &node, &[], &[]).window_name, "");
}
