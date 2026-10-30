use std::collections::VecDeque;

use mpx::tui::command::COMMAND_NAMES;
use mpx::tui::prompt::{PromptKind, PromptRun, PromptState, Question, expand_template};

fn typed(text: &str) -> PromptState {
    let mut p = PromptState::new("");
    for c in text.chars() {
        p.insert(c);
    }
    p
}

#[test]
fn insert_and_backspace_edit_at_cursor() {
    let mut p = typed("splt");
    p.left();
    p.insert('i');
    assert_eq!(p.text(), "split");
    p.end();
    p.backspace();
    assert_eq!(p.text(), "spli");
    p.home();
    p.backspace();
    assert_eq!(p.text(), "spli", "backspace at start is a no-op");
}

#[test]
fn delete_removes_character_under_cursor() {
    let mut p = typed("ab");
    p.home();
    p.delete();
    assert_eq!(p.text(), "b");
    assert_eq!(p.cursor(), 0);
}

#[test]
fn cursor_motion_is_clamped() {
    let mut p = typed("ab");
    p.right();
    assert_eq!(p.cursor(), 2);
    p.home();
    p.left();
    assert_eq!(p.cursor(), 0);
}

#[test]
fn prefilled_prompt_starts_at_end() {
    let mut p = PromptState::new("rename-window ");
    assert_eq!(p.cursor(), "rename-window ".chars().count());
    p.insert('x');
    assert_eq!(p.text(), "rename-window x");
}

#[test]
fn ctrl_u_clears_the_line() {
    let mut p = typed("select-pane -L");
    p.clear();
    assert_eq!(p.text(), "");
    assert_eq!(p.cursor(), 0);
}

#[test]
fn ctrl_w_deletes_the_previous_word() {
    let mut p = typed("resize-pane -L ");
    p.delete_word();
    assert_eq!(p.text(), "resize-pane ");
    p.delete_word();
    assert_eq!(p.text(), "");
}

#[test]
fn history_up_and_down_restore_the_draft() {
    let history = vec!["split-window".to_string(), "kill-pane".to_string()];
    let mut p = typed("nex");
    p.history_prev(&history);
    assert_eq!(p.text(), "kill-pane");
    p.history_prev(&history);
    assert_eq!(p.text(), "split-window");
    p.history_prev(&history);
    assert_eq!(p.text(), "split-window", "oldest entry stays put");
    p.history_next(&history);
    assert_eq!(p.text(), "kill-pane");
    p.history_next(&history);
    assert_eq!(p.text(), "nex", "past the newest entry the draft returns");
    p.history_next(&history);
    assert_eq!(p.text(), "nex");
}

#[test]
fn history_is_ignored_when_empty() {
    let mut p = typed("x");
    p.history_prev(&[]);
    assert_eq!(p.text(), "x");
}

#[test]
fn editing_leaves_history_browsing() {
    let history = vec!["kill-pane".to_string()];
    let mut p = PromptState::new("");
    p.history_prev(&history);
    p.insert('!');
    assert_eq!(p.text(), "kill-pane!");
    p.history_next(&history);
    assert_eq!(p.text(), "kill-pane!", "down does nothing once edited");
}

#[test]
fn tab_completes_a_unique_command_with_trailing_space() {
    let mut p = typed("rename-w");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "rename-window ");
}

#[test]
fn tab_extends_to_common_prefix_of_several_commands() {
    let mut p = typed("rena");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "rename");
    let mut p = typed("kill-");
    p.complete(COMMAND_NAMES);
    assert_eq!(
        p.text(),
        "kill-",
        "kill-pane, kill-server and kill-window share only this"
    );
    let mut p = typed("swap");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "swap-pane ");
}

#[test]
fn tab_on_ambiguous_short_name_leaves_text_alone() {
    let mut p = typed("q");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "q");
}

#[test]
fn tab_does_not_complete_arguments_or_unknown_names() {
    let mut p = typed("split-window -");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "split-window -");
    let mut p = typed("zzz");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "zzz");
}

#[test]
fn tab_cycles_through_matches_and_backtab_reverses() {
    let mut p = typed("kill-");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "kill-");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "kill-pane");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "kill-server");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "kill-window");
    p.complete_prev(COMMAND_NAMES);
    assert_eq!(p.text(), "kill-server");
}

fn question(label: &str, initial: &str) -> Question {
    Question {
        label: label.into(),
        initial: initial.into(),
        kind: PromptKind::Command,
        numeric: false,
    }
}

#[test]
fn placeholders_take_the_answers_by_number() {
    let answers = vec!["work".to_string(), "two".to_string()];
    assert_eq!(
        expand_template("rename-window -- '%%'", &answers),
        "rename-window -- 'work'"
    );
    assert_eq!(
        expand_template("join %1 %2 %3 %0", &answers),
        "join work two %3 %0"
    );
}

#[test]
fn triple_percent_quotes_the_answer_for_the_command_parser() {
    let answers = vec!["it's $HOME; \"x\" \\".to_string()];
    assert_eq!(
        expand_template("echo %%%", &answers),
        r#"echo it\'s \$HOME\; \"x\" \\"#
    );
}

#[test]
fn answers_fill_a_template_one_question_at_a_time() {
    let questions = VecDeque::from([question("from", "a"), question("to", "")]);
    let mut run = PromptRun::new(questions, Some("move '%1' '%2'".into()), false);
    assert_eq!(run.line.label(), "from");
    assert_eq!(
        run.line.text(),
        "a",
        "the first question starts with its initial text"
    );
    assert_eq!(run.answer("x".into()), None, "a second question follows");
    assert_eq!(run.line.label(), "to");
    assert_eq!(run.answer(" y ".into()), Some("move 'x' 'y'".into()));
}

#[test]
fn without_a_template_the_typed_line_is_the_answer() {
    let mut run = PromptRun::new(VecDeque::new(), None, false);
    assert_eq!(run.line.label(), ":");
    assert_eq!(
        run.answer("split-window".into()),
        Some("split-window".into())
    );
}

#[test]
fn a_template_prompt_without_a_text_asks_for_a_command() {
    let run = PromptRun::new(VecDeque::new(), Some("kill-pane".into()), true);
    assert_eq!(run.line.label(), "(command) ");
    assert!(run.single_key);
}

#[test]
fn numeric_prompts_accept_digits_only() {
    let mut p = PromptState::with("index", "", PromptKind::Target, true);
    for c in "1a2 3-".chars() {
        p.insert(c);
    }
    assert_eq!(p.text(), "123");
    assert_eq!(p.label(), "index");
}
