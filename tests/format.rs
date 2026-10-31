//! Format expansion (`#{...}`) and the variables panes and sessions provide.
use mpx::layout::{Rect, WindowTab};
use mpx::protocol::NodeInfo;
use mpx::format::{PaneView, Vars, expand, pane_vars, pane_view};

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
    let v = pane_vars(&pane_view("dev", 0, &node("p", 1), &[], &[]));
    assert_eq!(
        expand(
            "#{pane_tty} #{pane_start_command} #{pane_in_mode} #{version} #{client_session} #{session_attached}",
            &v
        ),
        "/dev/ttys004 zsh 0 3.4 dev 1"
    );
    assert_eq!(expand("#{pid}", &v), std::process::id().to_string());
    let host = expand("#{host}", &v);
    let short = expand("#{host_short}", &v);
    assert!(!short.contains('.'));
    assert!(host.starts_with(&short));
}

#[test]
fn window_counts_come_from_the_pane_view() {
    let p = PaneView {
        window_panes: 3,
        session_windows: 2,
        ..Default::default()
    };
    assert_eq!(expand("#{window_panes}/#{session_windows}", &pane_vars(&p)), "3/2");
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
