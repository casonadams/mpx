use mpx::format::expand;
use mpx::source::lines;

/// The config evaluator with formats expanded as a session with no pane would expand them.
fn run(text: &str) -> Result<Vec<String>, String> {
    lines(text, |fmt| {
        expand(
            fmt,
            &mpx::format::session_vars("main", Default::default(), 0),
        )
    })
    .map(|lines| lines.into_iter().map(|(_, text)| text).collect())
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let text = "# a comment\n\n  set -g mouse on\n";
    assert_eq!(run(text).unwrap(), vec!["set -g mouse on"]);
}

#[test]
fn backslash_joins_continued_lines() {
    let text = "bind x run-shell \\\n  'echo hi'\n";
    assert_eq!(run(text).unwrap(), vec!["bind x run-shell   'echo hi'"]);
}

#[test]
fn if_keeps_the_true_branch_only() {
    let text = "%if 1\nkeep-a\n%else\ndrop-a\n%endif\n%if 0\ndrop-b\n%else\nkeep-b\n%endif\n";
    assert_eq!(run(text).unwrap(), vec!["keep-a", "keep-b"]);
}

#[test]
fn elif_takes_the_first_true_condition() {
    let text = "%if 0\na\n%elif 1\nb\n%elif 1\nc\n%else\nd\n%endif\n";
    assert_eq!(run(text).unwrap(), vec!["b"]);
}

#[test]
fn conditions_are_formats() {
    let text = "%if #{==:#{session_name},main}\nyes\n%endif\n%if #{==:#{session_name},other}\nno\n%endif\n";
    assert_eq!(run(text).unwrap(), vec!["yes"]);
}

#[test]
fn nested_conditionals_follow_their_parent() {
    let text = "%if 0\n%if 1\nhidden\n%endif\n%endif\n%if 1\n%if 1\nshown\n%endif\n%endif\n";
    assert_eq!(run(text).unwrap(), vec!["shown"]);
}

#[test]
fn hidden_variables_expand_in_later_lines_and_conditions() {
    let text = "%hidden MODE=\"fast\"\n%if #{==:#{MODE},fast}\nset -g status-interval 1\n%endif\nset -g @mode #{MODE}\n";
    assert_eq!(
        run(text).unwrap(),
        vec!["set -g status-interval 1", "set -g @mode fast"]
    );
}

#[test]
fn hidden_in_a_false_branch_is_not_defined() {
    let text = "%if 0\n%hidden X=1\n%endif\nset -g a #{X}\n";
    assert_eq!(run(text).unwrap(), vec!["set -g a #{X}"]);
}

#[test]
fn line_numbers_name_the_first_line_of_a_command() {
    let text = "set -g a 1\n\\\n  set -g b 2\n";
    let got = lines(text, |f| f.to_string()).unwrap();
    assert_eq!(got[0].0, 1);
    assert_eq!(got.len(), 2);
}

#[test]
fn an_unclosed_if_is_an_error_at_its_line() {
    let err = run("set -g a 1\n%if 1\nset -g b 2\n").unwrap_err();
    assert!(err.contains("line 2") && err.contains("%endif"), "{err}");
}

#[test]
fn a_stray_endif_is_an_error() {
    let err = run("%endif\n").unwrap_err();
    assert!(err.contains("%endif without %if"), "{err}");
}

#[test]
fn an_unknown_directive_is_an_error() {
    let err = run("%bogus\n").unwrap_err();
    assert!(err.contains("unknown directive %bogus"), "{err}");
}
