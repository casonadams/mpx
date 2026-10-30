//! `send-keys -X` command names: the action each names, the errors a bad name or argument gives,
//! and the rectangle selection `rectangle-toggle` starts.
use mpx::tui::copy::{
    CopyAction, CopyOutcome, CopyState, CopyWhat, Motion, SearchDirection, SelectionKind,
    apply_action, parse_copy_command,
};
use mpx::tui::render::SelectionRange;
use mpx::vt::VtScreen;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn parse(name: &str, words: &[&str]) -> Result<CopyAction, String> {
    parse_copy_command(name, &args(words))
}

fn motion(motion: Motion) -> CopyAction {
    CopyAction::Move {
        motion,
        cancel_at_bottom: false,
    }
}

#[test]
fn cursor_and_line_names_map_to_their_motions() {
    let cases = [
        ("cursor-left", Motion::Left),
        ("cursor-right", Motion::Right),
        ("cursor-up", Motion::Up),
        ("cursor-down", Motion::Down),
        ("start-of-line", Motion::LineStart),
        ("end-of-line", Motion::LineEnd),
        ("back-to-indentation", Motion::BackToIndentation),
        ("next-word", Motion::NextWord { big: false }),
        ("next-space", Motion::NextWord { big: true }),
        ("previous-word", Motion::PreviousWord { big: false }),
        ("previous-space", Motion::PreviousWord { big: true }),
        ("next-word-end", Motion::NextWordEnd { big: false }),
        ("next-space-end", Motion::NextWordEnd { big: true }),
        ("forward-four-columns", Motion::FourForward),
        ("back-four-columns", Motion::FourBack),
        ("top-line", Motion::TopLine),
        ("middle-line", Motion::MiddleLine),
        ("bottom-line", Motion::BottomLine),
        ("history-top", Motion::HistoryTop),
        ("history-bottom", Motion::HistoryBottom),
        ("page-up", Motion::PageUp),
        ("page-down", Motion::PageDown),
        ("halfpage-up", Motion::HalfPageUp),
        ("halfpage-down", Motion::HalfPageDown),
        ("scroll-up", Motion::ScrollUp),
        ("scroll-down", Motion::ScrollDown),
    ];
    for (name, expected) in cases {
        assert_eq!(parse(name, &[]), Ok(motion(expected)), "{name}");
    }
}

#[test]
fn cancel_variants_leave_copy_mode_at_the_bottom_of_the_screen() {
    assert_eq!(
        parse("cursor-down-and-cancel", &[]),
        Ok(CopyAction::Move {
            motion: Motion::Down,
            cancel_at_bottom: true,
        })
    );
    assert_eq!(
        parse("halfpage-down-and-cancel", &[]),
        Ok(CopyAction::Move {
            motion: Motion::HalfPageDown,
            cancel_at_bottom: true,
        })
    );
    assert_eq!(parse("cancel", &[]), Ok(CopyAction::Cancel));
}

#[test]
fn selection_names_map_to_their_actions() {
    let cases = [
        ("begin-selection", CopyAction::BeginSelection),
        ("select-line", CopyAction::SelectLine),
        ("select-word", CopyAction::SelectWord),
        ("clear-selection", CopyAction::ClearSelection),
        ("other-end", CopyAction::OtherEnd),
        ("rectangle-toggle", CopyAction::RectangleToggle),
    ];
    for (name, expected) in cases {
        assert_eq!(parse(name, &[]), Ok(expected), "{name}");
    }
}

#[test]
fn copy_names_say_what_they_copy_and_whether_they_cancel_or_clear() {
    let copy = |what, cancel, clear| CopyAction::Copy {
        what,
        cancel,
        clear,
        pipe: None,
    };
    assert_eq!(
        parse("copy-selection", &[]),
        Ok(copy(CopyWhat::Selection, false, true))
    );
    assert_eq!(
        parse("copy-selection-no-clear", &[]),
        Ok(copy(CopyWhat::Selection, false, false))
    );
    assert_eq!(
        parse("copy-selection-and-cancel", &[]),
        Ok(copy(CopyWhat::Selection, true, true))
    );
    assert_eq!(
        parse("copy-selection-or-line-and-cancel", &[]),
        Ok(copy(CopyWhat::SelectionOrLine, true, false))
    );
    assert_eq!(
        parse("copy-line", &[]),
        Ok(copy(CopyWhat::Line, false, false))
    );
    assert_eq!(
        parse("copy-line-and-cancel", &[]),
        Ok(copy(CopyWhat::Line, true, false))
    );
    assert_eq!(
        parse("copy-end-of-line", &[]),
        Ok(copy(CopyWhat::EndOfLine, false, false))
    );
    assert_eq!(
        parse("copy-end-of-line-and-cancel", &[]),
        Ok(copy(CopyWhat::EndOfLine, true, false))
    );
    assert_eq!(
        parse("copy-selection-if-any", &[]),
        Ok(CopyAction::CopyIfSelected)
    );
}

#[test]
fn copy_pipe_names_keep_the_shell_command_from_the_arguments() {
    let piped = |what, cancel, clear| CopyAction::Copy {
        what,
        cancel,
        clear,
        pipe: Some("cat > out.txt".to_string()),
    };
    let words = ["cat", ">", "out.txt"];
    assert_eq!(
        parse("copy-pipe", &words),
        Ok(piped(CopyWhat::Selection, false, true))
    );
    assert_eq!(
        parse("copy-pipe-no-clear", &words),
        Ok(piped(CopyWhat::Selection, false, false))
    );
    assert_eq!(
        parse("copy-pipe-and-cancel", &words),
        Ok(piped(CopyWhat::Selection, true, true))
    );
    assert_eq!(
        parse("copy-pipe-line", &words),
        Ok(piped(CopyWhat::Line, false, false))
    );
    assert_eq!(
        parse("copy-pipe-line-and-cancel", &words),
        Ok(piped(CopyWhat::Line, true, false))
    );
    assert_eq!(
        parse("copy-pipe-end-of-line", &words),
        Ok(piped(CopyWhat::EndOfLine, false, false))
    );
    assert_eq!(
        parse("copy-pipe-end-of-line-and-cancel", &words),
        Ok(piped(CopyWhat::EndOfLine, true, false))
    );
}

#[test]
fn search_names_take_their_text_and_direction() {
    assert_eq!(
        parse("search-forward", &["two", "words"]),
        Ok(CopyAction::SearchText {
            direction: SearchDirection::Down,
            query: "two words".to_string(),
        })
    );
    assert_eq!(
        parse("search-backward", &["x"]),
        Ok(CopyAction::SearchText {
            direction: SearchDirection::Up,
            query: "x".to_string(),
        })
    );
    assert_eq!(
        parse("search-again", &[]),
        Ok(CopyAction::SearchAgain { reverse: false })
    );
    assert_eq!(
        parse("search-reverse", &[]),
        Ok(CopyAction::SearchAgain { reverse: true })
    );
    assert_eq!(
        parse("search-prompt-forward", &[]),
        Ok(CopyAction::SearchPrompt(SearchDirection::Down))
    );
    assert_eq!(
        parse("search-prompt-backward", &[]),
        Ok(CopyAction::SearchPrompt(SearchDirection::Up))
    );
}

#[test]
fn jump_names_take_a_character_and_say_direction_and_placement() {
    let jump = |forward, to, ch| Ok(motion(Motion::Jump { forward, to, ch }));
    assert_eq!(parse("jump-forward", &["x"]), jump(true, false, 'x'));
    assert_eq!(parse("jump-backward", &["x"]), jump(false, false, 'x'));
    assert_eq!(parse("jump-to-forward", &["y"]), jump(true, true, 'y'));
    assert_eq!(parse("jump-to-backward", &["y"]), jump(false, true, 'y'));
}

#[test]
fn goto_line_takes_a_line_number() {
    assert_eq!(
        parse("goto-line", &["12"]),
        Ok(motion(Motion::GotoLine(12)))
    );
}

#[test]
fn a_name_that_needs_an_argument_without_one_is_an_error() {
    assert_eq!(
        parse("search-forward", &[]),
        Err("send-keys -X search-forward: needs an argument".to_string())
    );
    assert_eq!(
        parse("search-backward", &[]),
        Err("send-keys -X search-backward: needs an argument".to_string())
    );
    assert_eq!(
        parse("jump-forward", &[]),
        Err("send-keys -X jump-forward: needs a character".to_string())
    );
    assert_eq!(
        parse("jump-to-backward", &[]),
        Err("send-keys -X jump-to-backward: needs a character".to_string())
    );
}

#[test]
fn goto_line_rejects_what_is_not_a_line_number() {
    assert_eq!(
        parse("goto-line", &["abc"]),
        Err("send-keys -X goto-line: invalid line 'abc'".to_string())
    );
    assert_eq!(
        parse("goto-line", &[]),
        Err("send-keys -X goto-line: invalid line ''".to_string())
    );
}

#[test]
fn names_mpx_does_not_implement_are_refused_by_name() {
    for name in [
        "bogus",
        "cursor-sideways",
        "copy-selection-now",
        "begin-selection-x",
        "",
    ] {
        assert_eq!(
            parse(name, &[]),
            Err(format!("copy-mode command not supported: {name}")),
            "{name:?}"
        );
    }
}

/// A two-row screen: `hello` on row 0 and `second` on row 1.
fn screen() -> VtScreen {
    let mut vt = VtScreen::new(10, 3);
    vt.feed(b"hello\r\nsecond");
    vt
}

/// Copy mode at the top-left cell of `vt`.
fn at_origin(vt: &VtScreen) -> CopyState {
    let mut cs = CopyState::enter(Some(vt.grid()));
    cs.cursor_x = 0;
    cs.cursor_y = 0;
    cs.anchor_x = 0;
    cs.anchor_abs_y = vt.grid().abs_line(0, 0);
    cs
}

#[test]
fn rectangle_toggle_selects_the_columns_between_the_corners_on_every_row() {
    let vt = screen();
    let pane = (10, 3);
    let mut cs = at_origin(&vt);
    apply_action(&mut cs, &CopyAction::RectangleToggle, Some(vt.grid()), pane);
    assert_eq!(cs.mode, SelectionKind::Rectangle);
    apply_action(&mut cs, &CopyAction::BeginSelection, Some(vt.grid()), pane);
    assert_eq!(
        cs.mode,
        SelectionKind::Rectangle,
        "begin-selection keeps the rectangle"
    );

    for _ in 0..4 {
        apply_action(&mut cs, &motion(Motion::Right), Some(vt.grid()), pane);
    }
    apply_action(&mut cs, &motion(Motion::Down), Some(vt.grid()), pane);
    let range = cs.selection_range(vt.grid());
    assert!(
        matches!(range, Some(SelectionRange::Rectangle { .. })),
        "{range:?}"
    );
    let picked = |x, y| range.is_some_and(|r| r.contains_abs(x, y));
    assert!(picked(0, 0) && picked(4, 1), "corners are selected");
    assert!(
        !picked(5, 0) && !picked(0, 2),
        "outside the rectangle is not"
    );

    let outcome = apply_action(
        &mut cs,
        &CopyAction::Copy {
            what: CopyWhat::Selection,
            cancel: true,
            clear: true,
            pipe: None,
        },
        Some(vt.grid()),
        pane,
    );
    assert_eq!(outcome, CopyOutcome::Yank("hello\nsecon".to_string()));
}

#[test]
fn rectangle_toggle_twice_returns_to_a_character_selection_with_the_same_anchor() {
    let vt = screen();
    let pane = (10, 3);
    let mut cs = at_origin(&vt);
    apply_action(&mut cs, &CopyAction::BeginSelection, Some(vt.grid()), pane);
    apply_action(&mut cs, &CopyAction::RectangleToggle, Some(vt.grid()), pane);
    assert_eq!(cs.mode, SelectionKind::Rectangle);
    apply_action(&mut cs, &CopyAction::RectangleToggle, Some(vt.grid()), pane);
    assert_eq!(cs.mode, SelectionKind::Character);
    assert_eq!(
        (cs.anchor_x, cs.anchor_abs_y),
        (0, vt.grid().abs_line(0, 0))
    );
}

#[test]
fn rectangle_toggle_without_a_grid_still_changes_the_selection_kind() {
    let mut cs = CopyState::enter(None);
    apply_action(&mut cs, &CopyAction::RectangleToggle, None, (10, 3));
    assert_eq!(cs.mode, SelectionKind::Rectangle);
}
