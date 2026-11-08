//! Format expansion (`#{...}`) and the variables panes and sessions provide.
use mpx::format::{
    DEFAULT_BUFFER_FORMAT, Lists, PaneView, SessionTimes, Vars, buffer_sample, buffer_vars, expand,
    expand_with, pane_vars, pane_view, session_vars,
};
use mpx::layout::{Rect, WindowTab};
use mpx::protocol::{Buffer, NodeInfo};

fn vars(pairs: &[(&'static str, &str)]) -> Vars {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

/// Asserts each `(format, want)` pair expands under `v`; the message names the failing format.
fn assert_expands(v: &Vars, cases: &[(&str, &str)]) {
    for (format, want) in cases {
        assert_eq!(expand(format, v), *want, "format {format:?}");
    }
}

fn base_vars() -> Vars {
    vars(&[
        ("session_name", "main"),
        ("window_index", "2"),
        ("pane_index", "1"),
        ("window_name", "dev"),
        ("pane_title", "abcdef"),
        ("pane_id", "%3"),
        ("flag", "1"),
        ("off", "0"),
        ("empty", ""),
        ("name", "foo boo"),
        ("path", "/work/src/"),
        ("file", "file"),
        ("spaced", "a b$"),
        ("fmt", "#{session_name}!"),
        ("loop", "#{E:loop}"),
        ("uni", "é"),
        ("root", "/x"),
    ])
}

#[test]
fn aliases_and_plain_references_expand() {
    let v = base_vars();
    assert_eq!(expand("#S:#I.#P #W #T #D", &v), "main:2.1 dev abcdef %3");
    assert_eq!(expand("#{session_name}/#{pane_title}", &v), "main/abcdef");
}

#[test]
fn unknown_variables_expand_to_empty() {
    let v = base_vars();
    assert_eq!(expand("[#{nope}]", &v), "[]");
    assert_eq!(expand("[#{pane_zoomed}]", &v), "[]");
}

#[test]
fn escapes_produce_literal_hash_comma_and_brace() {
    assert_expands(
        &base_vars(),
        &[
            ("100##", "100#"),
            ("a#,b#}", "a,b}"),
            ("#{?flag,a#,b,c}", "a,b"),
            ("#{?flag,a#}b,c}", "a}b"),
        ],
    );
}

#[test]
fn conditional_picks_branch_by_truthiness() {
    assert_expands(
        &base_vars(),
        &[
            ("#{?flag,yes,no}", "yes"),
            ("#{?off,yes,no}", "no"),
            ("#{?empty,yes,no}", "no"),
            ("#{?unknown,yes,no}", "no"),
            ("#{?off,x}", ""),
        ],
    );
}

#[test]
fn conditional_chains_pairs_before_else() {
    let v = base_vars();
    assert_eq!(expand("#{?off,a,flag,b,c}", &v), "b");
    assert_eq!(expand("#{?off,a,empty,b,c}", &v), "c");
}

#[test]
fn conditional_condition_and_branches_may_be_nested_formats() {
    let v = base_vars();
    assert_eq!(expand("#{?#{==:a,a},yes,no}", &v), "yes");
    assert_eq!(expand("#{?#{==:a,b},x,y}", &v), "y");
    assert_eq!(
        expand("#{?flag,#{pane_id}-#{session_name},no}", &v),
        "%3-main"
    );
}

#[test]
fn comparisons_compare_strings_and_integers() {
    let v = base_vars();
    assert_eq!(expand("#{==:a,a}", &v), "1");
    assert_eq!(expand("#{==:a,b}", &v), "0");
    assert_eq!(expand("#{!=:a,b}", &v), "1");
    assert_eq!(expand("#{<:2,10}", &v), "1");
    assert_eq!(expand("#{<:b,a}", &v), "0");
    assert_eq!(expand("#{>:b,a}", &v), "1");
    assert_eq!(expand("#{<=:2,2}", &v), "1");
    assert_eq!(expand("#{>=:1,2}", &v), "0");
    assert_eq!(expand("#{==:#{session_name},main}", &v), "1");
}

#[test]
fn boolean_operators_use_truthiness() {
    let v = base_vars();
    assert_eq!(expand("#{&&:1,0}", &v), "0");
    assert_eq!(expand("#{&&:1,#{flag}}", &v), "1");
    assert_eq!(expand("#{||:0,1}", &v), "1");
    assert_eq!(expand("#{||:0,#{empty}}", &v), "0");
    assert_eq!(expand("#{!:}", &v), "1");
    assert_eq!(expand("#{!:x}", &v), "0");
}

#[test]
fn truncation_keeps_leading_or_trailing_characters() {
    let v = base_vars();
    assert_eq!(expand("#{=3:pane_title}", &v), "abc");
    assert_eq!(expand("#{=-2:pane_title}", &v), "ef");
    assert_eq!(expand("#{=10:pane_title}", &v), "abcdef");
    assert_eq!(expand("#{=2:#{pane_id}}", &v), "%3");
}

#[test]
fn substitution_replaces_all_matches() {
    let v = base_vars();
    assert_eq!(expand("#{s/o/0/:name}", &v), "f00 b00");
    assert_eq!(expand("#{s/[0-9]+/N/:#{pane_id}}", &v), "%N");
    assert_eq!(expand("#{s/FOO/x/i:name}", &v), "x boo");
    assert_eq!(expand("#{s/(/x/:spaced}", &v), "a b$");
    let literal = vars(&[("v", "a(b")]);
    assert_eq!(expand("#{s/(/x/:v}", &literal), "axb");
}

#[test]
fn basename_and_dirname_follow_posix_rules() {
    let v = base_vars();
    assert_eq!(expand("#{b:path}", &v), "src");
    assert_eq!(expand("#{d:path}", &v), "/work");
    assert_eq!(expand("#{b:file}", &v), "file");
    assert_eq!(expand("#{d:file}", &v), ".");
    assert_eq!(expand("#{d:root}", &v), "/");
}

#[test]
fn basename_splits_on_backslashes_only_where_they_separate_paths() {
    let v = vars(&[("win", r"C:\work\app\")]);
    let expected = if cfg!(windows) {
        "app"
    } else {
        r"C:\work\app\"
    };
    assert_eq!(expand("#{b:win}", &v), expected);
}

#[test]
fn literal_modifier_outputs_its_text_unexpanded_except_escapes() {
    let v = base_vars();
    assert_eq!(expand("#{l:#{pane_id}}", &v), "#{pane_id}");
    assert_eq!(expand("#{l:a##b}", &v), "a#b");
    assert_eq!(expand("#{l:x#,y}", &v), "x,y");
}

#[test]
fn expand_modifier_formats_the_variable_value_again() {
    let v = base_vars();
    assert_eq!(expand("#{E:fmt}", &v), "main!");
    assert_eq!(expand("#{E:loop}", &v), "");
}

#[test]
fn length_and_shell_quote_modifiers() {
    let v = base_vars();
    assert_eq!(expand("#{n:file}", &v), "4");
    assert_eq!(expand("#{n:uni}", &v), "1");
    assert_eq!(expand("#{q:spaced}", &v), "a\\ b\\$");
}

#[test]
fn nested_braces_and_multibyte_text_survive() {
    let v = base_vars();
    assert_eq!(expand("é#{session_name}é", &v), "émainé");
    assert_eq!(expand("#{?flag,#{?off,x,y},z}", &v), "y");
}

#[test]
fn malformed_input_is_left_as_text_without_panicking() {
    let v = base_vars();
    assert_eq!(expand("x #{pane_id", &v), "x #{pane_id");
    assert_eq!(expand("#{", &v), "#{");
    assert_eq!(expand("trailing #", &v), "trailing #");
    assert_eq!(expand("#{?a,#{b}", &v), "#{?a,");
    assert_eq!(expand("#{s/unclosed", &v), "#{s/unclosed");
    assert_eq!(expand("#{=x:pane_title}", &v), "");
}

fn node(name: &str, window: usize) -> NodeInfo {
    let mut n = NodeInfo::new(name.into(), true, "zsh".into());
    n.id = 7;
    n.window = Some(window);
    n.tty = Some("ttys004".into());
    n.cwd = Some("/work".into());
    n
}

fn tab(index: usize) -> WindowTab {
    WindowTab {
        index,
        name: format!("w{index}"),
        ..Default::default()
    }
}

#[test]
fn window_flags_follow_tmux_order() {
    let tabs = vec![
        WindowTab {
            is_last: true,
            ..tab(0)
        },
        WindowTab {
            is_active: true,
            has_activity: true,
            has_bell: true,
            has_marked: true,
            ..tab(1)
        },
    ];
    let active = pane_vars(&pane_view("dev", 0, &node("p", 1), &[], &tabs));
    assert_eq!(
        expand(
            "#{window_flags}|#{window_active}|#{session_windows}|#{window_name}",
            &active
        ),
        "#!*M|1|2|w1"
    );
    let last = pane_vars(&pane_view("dev", 0, &node("p", 0), &[], &tabs));
    assert_eq!(expand("#{window_flags}|#{window_active}", &last), "-|0");
}

#[test]
fn pane_rectangles_come_from_the_layout_when_laid_out() {
    let rects = vec![("p".to_string(), Rect::new(2, 3, 10, 5), true)];
    let laid_out = pane_vars(&pane_view("dev", 0, &node("p", 1), &rects, &[]));
    assert_eq!(
        expand(
            "#{pane_left},#{pane_top},#{pane_right},#{pane_bottom}",
            &laid_out
        ),
        "2,3,11,7"
    );
    let hidden = pane_vars(&pane_view("dev", 0, &node("p", 1), &[], &[]));
    assert_eq!(expand("[#{pane_left}][#{pane_bottom}]", &hidden), "[][]");
}

#[test]
fn pane_identity_and_fixed_session_values_expand() {
    let view = mpx::format::PaneView {
        session_attached: 2,
        ..pane_view("dev", 0, &node("p", 1), &[], &[])
    };
    let v = pane_vars(&view);
    assert_eq!(
        expand(
            "#{pane_tty} #{pane_start_command} #{pane_in_mode} #{version} #{client_session} #{session_attached}",
            &v
        ),
        "/dev/ttys004 zsh 0 3.4 dev 2"
    );
    assert_eq!(expand("#{pid}", &v), std::process::id().to_string());
    let host = expand("#{host}", &v);
    let short = expand("#{host_short}", &v);
    assert!(!short.contains('.'));
    assert!(host.starts_with(&short));
}

#[test]
fn session_time_variables_come_from_the_daemon_start_and_restore() {
    let started = 1_700_000_123_456;
    let fresh = PaneView {
        times: SessionTimes {
            started_ms: started,
            restored_ms: None,
        },
        ..pane_view("dev", 0, &node("p", 1), &[], &[])
    };
    assert_eq!(
        expand(
            "#{session_id} #{session_created} #{start_time}",
            &pane_vars(&fresh)
        ),
        "$1700000123456 1700000123 1700000123"
    );
    let restored = PaneView {
        times: SessionTimes {
            started_ms: started,
            restored_ms: Some(1_700_000_999_999),
        },
        ..pane_view("dev", 0, &node("p", 1), &[], &[])
    };
    assert_eq!(
        expand("#{session_created} #{start_time}", &pane_vars(&restored)),
        "1700000999 1700000123"
    );
}

#[test]
fn session_time_variables_expand_with_no_pane() {
    let times = SessionTimes {
        started_ms: 5_000,
        restored_ms: None,
    };
    let v = session_vars("dev", times, 0);
    assert_eq!(
        expand("#{session_id} #{session_created} #{start_time}", &v),
        "$5000 5 5"
    );
}

#[test]
fn window_counts_come_from_the_pane_view() {
    let p = PaneView {
        window_panes: 3,
        session_windows: 2,
        ..Default::default()
    };
    assert_eq!(
        expand("#{window_panes}/#{session_windows}", &pane_vars(&p)),
        "3/2"
    );
}

#[test]
fn zoom_is_a_z_flag_after_the_active_flag_and_its_own_variable() {
    let tabs = vec![WindowTab {
        is_active: true,
        zoomed: true,
        has_marked: true,
        ..tab(1)
    }];
    let vars = pane_vars(&pane_view("dev", 0, &node("p", 1), &[], &tabs));
    assert_eq!(
        expand("#{window_flags}|#{window_zoomed_flag}", &vars),
        "*ZM|1"
    );
    let unzoomed = vec![WindowTab {
        is_active: true,
        ..tab(1)
    }];
    let vars = pane_vars(&pane_view("dev", 0, &node("p", 1), &[], &unzoomed));
    assert_eq!(
        expand("#{window_flags}|#{window_zoomed_flag}", &vars),
        "*|0"
    );
}

#[test]
fn pane_active_is_each_windows_focused_pane() {
    let tabs = vec![
        WindowTab {
            focused: Some("p".into()),
            ..tab(0)
        },
        WindowTab {
            is_active: true,
            focused: Some("q".into()),
            ..tab(1)
        },
    ];
    let active = |name: &str, window: usize| {
        let vars = pane_vars(&pane_view("dev", 0, &node(name, window), &[], &tabs));
        expand("#{pane_active}", &vars)
    };
    assert_eq!(
        active("p", 0),
        "1",
        "the focused pane of an inactive window"
    );
    assert_eq!(active("q", 1), "1");
    assert_eq!(active("x", 1), "0");
}

#[test]
fn the_list_buffers_format_describes_each_buffer() {
    let buffer = Buffer {
        name: "buffer0000".into(),
        data: "hello\nworld".into(),
    };
    let vars = buffer_vars(&buffer);
    assert_eq!(
        expand(DEFAULT_BUFFER_FORMAT, &vars),
        "buffer0000: 11 bytes: \"hello\\nworld\""
    );
    assert_eq!(
        expand("#{buffer_name}=#{buffer_size}", &vars),
        "buffer0000=11"
    );
}

#[test]
fn buffer_samples_keep_fifty_characters_and_escape_control_bytes() {
    assert_eq!(
        buffer_sample(&"x".repeat(60)),
        format!("{}...", "x".repeat(50))
    );
    assert_eq!(buffer_sample("a\tb\u{7}"), "a\\tb?");
}

#[test]
fn match_and_arithmetic_operators_compute_their_values() {
    assert_expands(
        &vars(&[("name", "build-42")]),
        &[
            ("#{m:build*,#{name}}", "1"),
            ("#{m:b?ild-4[0-9],#{name}}", "1"),
            ("#{m:[!b]*,#{name}}", "0"),
            ("#{m:BUILD*,#{name}}", "0"),
            ("#{m/i:BUILD*,#{name}}", "1"),
            ("#{m/r:^build-\\d+$,#{name}}", "1"),
            ("#{m/r:^deploy,#{name}}", "0"),
            ("#{m/r:(,#{name}}", "0"),
        ],
    );
    assert_expands(
        &base_vars(),
        &[
            ("#{e|+:2,3}", "5"),
            ("#{e|-:2,5}", "-3"),
            ("#{e|*:4,6}", "24"),
            ("#{e|/:7,2}", "3"),
            ("#{e|%:7,2}", "1"),
            ("#{e|/:7,0}", "0"),
            ("#{e|m:4,9}", "4"),
            ("#{e|M:4,9}", "9"),
            ("#{e|+|f|2:1.5,2.25}", "3.75"),
            ("#{e|+:#{window_index},1}", "3"),
        ],
    );
}

#[test]
fn time_operator_prints_local_ctime_layout() {
    let v = vars(&[("now", "0"), ("bad", "soon")]);
    let local = expand("#{t:now}", &v);
    assert_eq!(local.split_whitespace().count(), 5, "{local:?}");
    assert!(local.ends_with(|c: char| c.is_ascii_digit()), "{local:?}");
    assert_eq!(expand("#{t:bad}", &v), "");
}

#[test]
fn window_flags_and_pane_edges_are_exposed_as_variables() {
    let tabs = vec![WindowTab {
        is_active: true,
        has_activity: true,
        has_silence: true,
        has_bell: true,
        is_last: true,
        ..tab(0)
    }];
    let left = Rect::new(0, 0, 40, 10);
    let right = Rect::new(40, 0, 40, 10);
    let rects = vec![
        ("a".to_string(), left, true),
        ("b".to_string(), right, false),
    ];
    let a = pane_vars(&pane_view("dev", 0, &node("a", 0), &rects, &tabs));
    assert_expands(
        &a,
        &[
            (
                "#{window_activity_flag}#{window_bell_flag}#{window_silence_flag}#{window_last_flag}",
                "1111",
            ),
            ("#{window_marked_flag}", "0"),
            (
                "#{pane_at_left}#{pane_at_top}#{pane_at_right}#{pane_at_bottom}",
                "1101",
            ),
            ("#{window_flags}", "#!~*-"),
        ],
    );
    let b = pane_vars(&pane_view("dev", 0, &node("b", 0), &rects, &tabs));
    assert_expands(&b, &[("#{pane_at_left}#{pane_at_right}", "01")]);
}

#[test]
fn window_pane_and_nested_loops_expand_body_once_per_item() {
    let windows = Lists {
        windows: vec![
            vars(&[("window_index", "1"), ("window_name", "shell")]),
            vars(&[("window_index", "2"), ("window_name", "logs")]),
        ],
        ..Lists::default()
    };
    let panes = Lists {
        panes: vec![
            vars(&[("pane_index", "0"), ("pane_active", "1")]),
            vars(&[("pane_index", "1"), ("pane_active", "0")]),
        ],
        ..Lists::default()
    };
    let nested = Lists {
        windows: vec![
            vars(&[("window_active", "1"), ("window_name", "shell")]),
            vars(&[("window_active", "0"), ("window_name", "logs")]),
        ],
        ..Lists::default()
    };
    let cases: &[(&Lists, &str, &str)] = &[
        (
            &windows,
            "#{W:#{window_index}=#{window_name} }",
            "1=shell 2=logs ",
        ),
        (&panes, "#{P:#{pane_index}#{?pane_active,*,}}", "0*1"),
        (
            &nested,
            "#{W:#{?window_active,[#{window_name}],#{window_name}}}",
            "[shell]logs",
        ),
        (&nested, "#{W:a#,b#}c}", "a,b}ca,b}c"),
    ];
    for (lists, format, want) in cases {
        assert_eq!(
            expand_with(format, &Vars::new(), lists),
            *want,
            "format {format:?}"
        );
    }
}

#[test]
fn session_loop_expands_once_for_the_session() {
    let lists = Lists {
        sessions: vec![vars(&[("session_name", "work")])],
        ..Lists::default()
    };
    assert_eq!(
        expand_with("#{S:#{session_name}}", &Vars::new(), &lists),
        "work"
    );
}

#[test]
fn loops_over_empty_lists_expand_to_nothing() {
    assert_eq!(
        expand(
            "x#{W:#{window_index}}y#{P:#{pane_index}}z#{S:#{session_name}}",
            &Vars::new()
        ),
        "xyz"
    );
}

#[test]
fn cursor_and_history_come_from_the_node_when_it_is_a_terminal() {
    let mut terminal = node("p", 0);
    terminal.cursor = Some((3, 1));
    terminal.history_size = Some(40);
    let v = pane_vars(&pane_view("dev", 0, &terminal, &[], &[]));
    assert_expands(&v, &[("#{cursor_x},#{cursor_y},#{history_size}", "3,1,40")]);
    let task = pane_vars(&pane_view("dev", 0, &node("t", 0), &[], &[]));
    assert_expands(&task, &[("#{cursor_x}|#{history_size}|", "||")]);
}

#[test]
fn pretty_time_shows_the_clock_for_recent_times_and_the_day_for_older_ones() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let stamp = now.to_string();
    let v = vars(&[("now", stamp.as_str()), ("old", "1000000000"), ("bad", "x")]);
    let recent = expand("#{t/p:now}", &v);
    assert_eq!(recent.len(), 5, "{recent:?}");
    assert_eq!(recent.as_bytes()[2], b':', "{recent:?}");
    let older = expand("#{t/p:old}", &v);
    assert!(
        older.len() > 5 && older.chars().next().is_some_and(char::is_alphabetic),
        "{older:?}"
    );
    assert_eq!(expand("#{t/p:bad}", &v), "");
}

#[test]
fn pane_input_off_is_a_flag_variable() {
    let off = node("p", 1);
    let on = NodeInfo {
        input_off: Some(true),
        ..node("p", 1)
    };
    assert_expands(
        &pane_vars(&pane_view("dev", 0, &on, &[], &[])),
        &[("#{pane_input_off}", "1")],
    );
    assert_expands(
        &pane_vars(&pane_view("dev", 0, &off, &[], &[])),
        &[("#{pane_input_off}", "0")],
    );
}

#[test]
fn session_loop_entry_carries_the_session_variables() {
    let times = SessionTimes {
        started_ms: 1_709_647_629_000,
        restored_ms: Some(1_709_647_700_000),
    };
    let tabs = [tab(0), tab(1)];
    let session = mpx::tui::statusline::SessionInfo {
        name: "work",
        times,
        attached: 2,
    };
    let lists = mpx::tui::statusline::status_lists(session, &tabs, &[], false, 0);
    assert_eq!(
        expand_with(
            "#{S:#{session_name} #{session_id} #{session_created} #{start_time} #{session_attached} #{session_windows}}",
            &Vars::new(),
            &lists,
        ),
        "work $1709647629000 1709647700 1709647629 2 2",
    );
}
