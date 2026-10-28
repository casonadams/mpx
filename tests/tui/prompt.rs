use mpx::tui::command::COMMAND_NAMES;
use mpx::tui::prompt::PromptState;

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
    let mut p = typed("rena");
    p.complete(COMMAND_NAMES);
    assert_eq!(p.text(), "rename-window ");
}

#[test]
fn tab_extends_to_common_prefix_of_several_commands() {
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
