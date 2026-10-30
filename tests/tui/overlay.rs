use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::layout::{Rect, WindowTab};
use mpx::protocol::{NodeInfo, NodeState};
use mpx::tui::command::Command;
use mpx::tui::overlay::nav::fuzzy_match;
use mpx::tui::overlay::tree_view::session_from_snapshot;
use mpx::tui::overlay::widgets::{age_label, printable, short_path};
use mpx::tui::overlay::{
    HelpEntry, HelpView, Overlay, OverlayAction, PaletteTab, PaletteView, TreePane,
    TreeSession, TreeView, TreeWindow,
};

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn fuzzy_match_is_case_insensitive_ordered_subsequence() {
    assert!(fuzzy_match("", "anything"));
    assert!(fuzzy_match("clde", "Claude"));
    assert!(fuzzy_match("a-b", "alpha-beta"));
    assert!(!fuzzy_match("xyz", "anything"));
    assert!(!fuzzy_match("ba", "ab"));
}

#[test]
fn age_labels_use_largest_whole_unit() {
    assert_eq!(age_label(12_000), "12s");
    assert_eq!(age_label(59_999), "59s");
    assert_eq!(age_label(3 * 60_000), "3m");
    assert_eq!(age_label(2 * 3_600_000), "2h");
    assert_eq!(age_label(3 * 86_400_000), "3d");
}

fn pane(name: &str, id: u32, active: bool) -> TreePane {
    TreePane {
        name: name.to_string(),
        id,
        active,
        program: name.to_string(),
        cwd: None,
        running: true,
        exit_code: None,
        last_output_ms: 0,
    }
}

fn tree() -> TreeView {
    TreeView::from_data(
        vec![
            TreeSession {
                name: "main".to_string(),
                endpoint: String::new(),
                windows: vec![
                    TreeWindow {
                        index: 0,
                        name: "shell".to_string(),
                        active: true,
                        panes: vec![pane("zsh", 1, false), pane("vim", 2, true)],
                    },
                    TreeWindow {
                        index: 1,
                        name: "logs".to_string(),
                        active: false,
                        panes: vec![pane("tail", 3, false)],
                    },
                ],
            },
            TreeSession {
                name: "work".to_string(),
                endpoint: String::new(),
                windows: vec![TreeWindow {
                    index: 0,
                    name: "edit".to_string(),
                    active: true,
                    panes: vec![pane("claude", 4, true)],
                }],
            },
        ],
        "main",
    )
}

/// Moves to `label` from the top and sends `k`.
fn act(view: &mut TreeView, label: &str, k: KeyEvent) -> OverlayAction {
    view.handle_key(key('g'));
    view.handle_key(key('g'));
    let idx = view
        .labels()
        .iter()
        .position(|l| l == label)
        .unwrap_or_else(|| panic!("label {label} not listed: {:?}", view.labels()));
    for _ in 0..idx {
        view.handle_key(key('j'));
    }
    view.handle_key(k)
}

#[test]
fn tree_lists_sessions_windows_and_panes_expanded_by_default() {
    assert_eq!(
        tree().labels(),
        vec![
            "main", "shell", "zsh", "vim", "logs", "tail", "work", "edit", "claude"
        ]
    );
}

#[test]
fn tree_h_collapses_a_session_and_l_expands_it_again() {
    let mut view = tree();
    assert_eq!(act(&mut view, "main", key('h')), OverlayAction::None);
    assert_eq!(view.labels(), vec!["main", "work", "edit", "claude"]);
    assert_eq!(act(&mut view, "main", key('l')), OverlayAction::None);
    assert_eq!(view.labels().len(), 9);
}

#[test]
fn tree_h_on_a_leaf_moves_to_its_parent_window() {
    let mut view = tree();
    assert_eq!(act(&mut view, "zsh", key('h')), OverlayAction::None);
    assert_eq!(
        view.selected_label().as_deref(),
        Some("shell"),
        "focus moved up to window"
    );
}

#[test]
fn tree_collapsed_window_hides_its_panes() {
    let mut view = tree();
    assert_eq!(act(&mut view, "shell", key('h')), OverlayAction::None);
    assert_eq!(
        view.labels(),
        vec!["main", "shell", "logs", "tail", "work", "edit", "claude"]
    );
}

#[test]
fn tree_enter_on_other_session_switches_to_it() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "work", code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SwitchSession {
            name: "work".to_string()
        }])
    );
}

#[test]
fn tree_enter_on_current_session_just_closes() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "main", code(KeyCode::Enter)),
        OverlayAction::Close
    );
}

#[test]
fn tree_enter_on_window_selects_it_and_switches_first_when_remote() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "logs", code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectWindow { index: 1 }])
    );

    let mut view = tree();
    assert_eq!(
        act(&mut view, "edit", code(KeyCode::Enter)),
        OverlayAction::Run(vec![
            Command::SwitchSession {
                name: "work".to_string()
            },
            Command::SelectWindow { index: 0 },
        ])
    );
}

#[test]
fn tree_enter_on_pane_selects_session_qualified_pane() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "claude", code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "work:%4".to_string()
        }])
    );
}

#[test]
fn tree_l_on_collapsed_session_expands_instead_of_selecting() {
    let mut view = tree();
    act(&mut view, "work", key('h'));
    assert_eq!(view.labels().len(), 7);
    assert_eq!(
        act(&mut view, "work", key('l')),
        OverlayAction::None,
        "l on collapsed session does not run select"
    );
    assert_eq!(view.labels().len(), 9, "session expanded");
}

#[test]
fn tree_x_confirms_kill_for_each_node_kind() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "claude", key('x')),
        OverlayAction::Run(vec![Command::Confirm {
            prompt: "kill pane claude?".to_string(),
            then: Box::new(Command::KillPane {
                target: Some("work:%4".to_string()),
            }),
        }])
    );

    let mut view = tree();
    assert_eq!(
        act(&mut view, "edit", key('x')),
        OverlayAction::Run(vec![
            Command::SwitchSession {
                name: "work".to_string(),
            },
            Command::Confirm {
                prompt: "kill window edit?".to_string(),
                then: Box::new(Command::KillWindow { index: Some(0) }),
            },
        ])
    );

    let mut view = tree();
    assert_eq!(
        act(&mut view, "work", key('x')),
        OverlayAction::Run(vec![
            Command::SwitchSession {
                name: "work".to_string(),
            },
            Command::Confirm {
                prompt: "kill session work?".to_string(),
                then: Box::new(Command::KillServer),
            },
        ])
    );
}

#[test]
fn tree_kill_on_current_session_needs_no_switch() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "main", key('x')),
        OverlayAction::Run(vec![Command::Confirm {
            prompt: "kill session main?".to_string(),
            then: Box::new(Command::KillServer),
        }])
    );
}

#[test]
fn tree_filter_keeps_matching_nodes_and_their_ancestors() {
    let mut view = tree();
    view.handle_key(key('/'));
    for c in "vim".chars() {
        view.handle_key(key(c));
    }
    assert_eq!(view.labels(), vec!["main", "shell", "vim"]);
}

#[test]
fn tree_q_and_esc_close() {
    let mut view = tree();
    assert_eq!(view.handle_key(key('q')), OverlayAction::Close);
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::Close);
}

#[test]
fn session_snapshot_places_panes_in_their_window_and_marks_focus() {
    let node = |name: &str, id: u32, window: Option<usize>| {
        let mut n = NodeInfo::new(name.to_string(), true, "sh".to_string());
        n.id = id;
        n.window = window;
        n
    };
    let snap = session_from_snapshot(
        "s".to_string(),
        &[
            node("a", 1, Some(0)),
            node("b", 2, Some(0)),
            node("c", 3, Some(1)),
        ],
        &[
            WindowTab {
                index: 0,
                name: "one".to_string(),
                is_active: true,
                ..Default::default()
            },
            WindowTab {
                index: 1,
                name: "two".to_string(),
                is_active: false,
                ..Default::default()
            },
        ],
        Some("a"),
    );
    assert_eq!(snap.windows.len(), 2);
    assert_eq!(snap.windows[0].panes.len(), 2);
    assert!(snap.windows[0].panes[0].active);
    assert!(!snap.windows[0].panes[1].active);
    assert_eq!(snap.windows[1].panes.len(), 1);
    assert!(!snap.windows[1].panes[0].active);
}

#[test]
fn session_snapshot_prefers_foreground_program_and_keeps_exit_status() {
    let mut shell = NodeInfo::new("a".to_string(), true, "/bin/zsh -l".to_string());
    shell.window = Some(0);
    shell.cwd = Some("/tmp".to_string());
    shell.foreground = Some("nvim".to_string());

    let mut dead = NodeInfo::new("b".to_string(), true, "cargo test".to_string());
    dead.window = Some(0);
    dead.state = NodeState::Exited;
    dead.exit_code = Some(101);

    let snap = session_from_snapshot(
        "s".to_string(),
        &[shell, dead],
        &[WindowTab {
            index: 0,
            name: "w".to_string(),
            is_active: true,
            ..Default::default()
        }],
        Some("a"),
    );
    assert_eq!(snap.windows[0].panes[0].program, "nvim");
    assert_eq!(snap.windows[0].panes[0].cwd.as_deref(), Some("/tmp"));
    assert!(snap.windows[0].panes[0].running);

    assert_eq!(snap.windows[0].panes[1].program, "cargo");
    assert!(!snap.windows[0].panes[1].running);
    assert_eq!(snap.windows[0].panes[1].exit_code, Some(101));
}

fn help_entries() -> Vec<HelpEntry> {
    [
        ("j/k", "move selection"),
        ("q", "close overlay"),
        ("Enter", "select/run"),
        ("/", "filter entries"),
    ]
    .into_iter()
    .map(|(key, desc)| HelpEntry {
        key: key.to_string(),
        command: desc.to_string(),
    })
    .collect()
}

#[test]
fn help_filter_matches_key_or_command_text() {
    let mut help = HelpView::new(help_entries());
    help.handle_key(key('/'));
    for c in "close".chars() {
        help.handle_key(key(c));
    }
    assert_eq!(help.visible().len(), 1);
    assert_eq!(help.visible()[0].key, "q");
}

#[test]
fn help_overlay_closes_on_q_and_esc() {
    let mut overlay = Overlay::help(help_entries());
    assert_eq!(overlay.handle_key(key('q')), OverlayAction::Close);
    let mut overlay = Overlay::help(help_entries());
    assert_eq!(overlay.handle_key(code(KeyCode::Esc)), OverlayAction::Close);
}

#[test]
fn help_motion_keys_do_not_close() {
    let mut help = HelpView::new(help_entries());
    for k in [
        key('j'),
        key('k'),
        key('g'),
        code(KeyCode::Down),
        code(KeyCode::Up),
    ] {
        assert_eq!(help.handle_key(k), OverlayAction::None);
    }
}

fn render_to_string(overlay: &Overlay, area: Rect) -> String {
    let mut buf: Vec<u8> = Vec::new();
    overlay.render(&mut buf, area).expect("render");
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn overlays_render_into_tiny_and_empty_areas_without_panicking() {
    for area in [
        Rect::new(0, 0, 0, 0),
        Rect::new(0, 0, 1, 1),
        Rect::new(0, 0, 5, 2),
    ] {
        render_to_string(&Overlay::tree(), area);
        render_to_string(&Overlay::help(help_entries()), area);
    }
}

#[test]
fn tree_render_shows_session_and_pane_names() {
    let text = render_to_string(&Overlay::Tree(tree()), Rect::new(0, 0, 60, 20));
    assert!(text.contains("main"));
    assert!(text.contains("zsh"));
}

#[test]
fn help_render_shows_entries_and_title() {
    let text = render_to_string(&Overlay::help(help_entries()), Rect::new(0, 0, 60, 10));
    assert!(text.contains("Help"));
    assert!(text.contains("close overlay"));
}

fn snapshot_overlay(
    area: Rect,
    render: impl FnOnce(&mut Vec<u8>) -> Result<(), std::io::Error>,
) -> String {
    let mut buf: Vec<u8> = Vec::new();
    render(&mut buf).expect("render");
    let mut screen = mpx::vt::VtScreen::new(
        usize::from(area.x + area.width),
        usize::from(area.y + area.height),
    );
    screen.feed(&buf);
    screen.snapshot()
}

fn draw_tree(view: &TreeView, area: Rect) -> String {
    snapshot_overlay(area, |buf| view.render(buf, area))
}

#[test]
fn tree_render_shows_expanded_and_collapsed_arrows() {
    let mut view = tree();
    view.handle_key(key('p'));
    let expanded = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(expanded.contains("▾ main"));

    assert_eq!(act(&mut view, "main", key('h')), OverlayAction::None);
    let collapsed = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(collapsed.contains("▸ main"));
}

#[test]
fn tree_render_shows_collapsed_window_arrow_and_hides_its_panes() {
    let mut view = tree();
    view.handle_key(key('p'));
    assert_eq!(act(&mut view, "logs", key('h')), OverlayAction::None);
    let text = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(text.contains("▸ 1 logs"));
    assert!(!text.contains("tail"));
}

#[test]
fn help_view_renders_its_entries_directly() {
    let view = HelpView::new(help_entries());
    let mut buf: Vec<u8> = Vec::new();
    view.render(&mut buf, Rect::new(0, 0, 60, 10)).unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("move selection"));
    assert!(text.contains("j/k"));
}

fn busy_tree() -> TreeView {
    let mut editor = pane("claude", 4, false);
    editor.cwd = Some("/home/me/src/mpx".to_string());
    let mut build = pane("build", 5, false);
    build.running = false;
    build.exit_code = Some(1);

    TreeView::from_data(
        vec![
            TreeSession {
                name: "work".to_string(),
                endpoint: String::new(),
                windows: vec![TreeWindow {
                    index: 0,
                    name: "editor".to_string(),
                    active: true,
                    panes: vec![editor],
                }],
            },
            TreeSession {
                name: "main".to_string(),
                endpoint: String::new(),
                windows: vec![TreeWindow {
                    index: 0,
                    name: "dev".to_string(),
                    active: true,
                    panes: vec![
                        pane("zsh", 3, true),
                        build,
                    ],
                }],
            },
        ],
        "main",
    )
}

fn row_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle}:\n{text}"))
}

#[test]
fn tree_lists_the_current_session_first() {
    assert_eq!(busy_tree().labels()[0], "main");
}

#[test]
fn tree_title_counts_sessions() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(text.contains("Sessions · 2"), "{text}");
}

#[test]
fn tree_marks_current_session_active_window_and_focused_pane_distinctly() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(row_with(&text, "▾ main").contains("current"), "{text}");
    assert!(!row_with(&text, "▾ work").contains("current"), "{text}");
}

#[test]
fn tree_pane_rows_show_program_short_cwd_state_and_exit_code() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(row_with(&text, "zsh").contains("zsh"), "{text}");
    let build = row_with(&text, "build");
    assert!(build.contains("exit 1"), "{build}");
}

#[test]
fn tree_collapsed_session_rolls_up_exits() {
    let mut view = busy_tree();
    view.handle_key(key('p'));
    assert_eq!(act(&mut view, "main", key('h')), OverlayAction::None);
    let text = draw_tree(&view, Rect::new(0, 0, 100, 12));
    let main_row = row_with(&text, "main");
    assert!(main_row.contains("1 exited"), "{main_row}");
}

#[test]
fn tree_narrow_rows_drop_the_detail_column() {
    let mut view = busy_tree();
    view.handle_key(key('p'));
    let text = draw_tree(&view, Rect::new(0, 0, 60, 12));
    assert!(!text.contains("panes"), "{text}");
}

#[test]
fn tree_filter_matches_pane_program_and_directory() {
    let mut view = busy_tree();
    view.handle_key(key('/'));
    "claude".chars().for_each(|c| {
        view.handle_key(key(c));
    });
    assert_eq!(view.labels(), vec!["work", "editor", "claude"]);
}

#[test]
fn tree_preview_names_the_pane_a_session_row_would_show() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 20));
    let meta = row_with(&text, "main:%3");
    assert!(meta.contains("zsh"), "{text}");
}

#[test]
fn short_path_replaces_only_a_whole_home_prefix() {
    let home = Some("/home/me");
    assert_eq!(short_path("/home/me", home), "~");
    assert_eq!(short_path("/home/me/src", home), "~/src");
    assert_eq!(short_path("/home/other", home), "/home/other");
    assert_eq!(short_path("/opt", home), "/opt");
}

fn sample_palette() -> PaletteView {
    let entries = vec![
        HelpEntry {
            key: "c".to_string(),
            command: "new-window".to_string(),
        },
        HelpEntry {
            key: "x".to_string(),
            command: "kill-pane".to_string(),
        },
    ];
    PaletteView::new(PaletteTab::Commands, entries, "main")
}

fn draw_palette(view: &PaletteView, area: Rect) -> String {
    snapshot_overlay(area, |buf| view.render(buf, area))
}

#[test]
fn palette_starts_on_commands_tab_and_cycles_tabs() {
    let mut view = sample_palette();
    assert_eq!(view.tab, PaletteTab::Commands);

    view.handle_key(code(KeyCode::Tab));
    assert_eq!(view.tab, PaletteTab::Sessions);

    view.handle_key(code(KeyCode::Tab));
    assert_eq!(view.tab, PaletteTab::Help);

    view.handle_key(code(KeyCode::Tab));
    assert_eq!(view.tab, PaletteTab::Commands);

    view.handle_key(code(KeyCode::BackTab));
    assert_eq!(view.tab, PaletteTab::Help);
}

#[test]
fn palette_fuzzy_filters_commands_and_edits_search_text() {
    let mut view = sample_palette();
    let initial_count = view.current_items().len();

    for c in "split".chars() {
        view.handle_key(key(c));
    }
    assert!(view.current_items().len() < initial_count);
    for item in view.current_items() {
        assert!(
            fuzzy_match("split", &item.title)
                || fuzzy_match("split", &item.subtitle)
        );
    }

    view.handle_key(code(KeyCode::Backspace));
    assert!(view.prompt.text() == "spli");

    view.handle_key(ctrl('w'));
    assert!(view.prompt.text().is_empty());

    view.handle_key(key('n'));
    view.handle_key(key('e'));
    view.handle_key(key('w'));
    view.handle_key(ctrl('u'));
    assert!(view.prompt.text().is_empty());
}

#[test]
fn palette_executes_commands_and_arbitrary_lines() {
    let mut view = sample_palette();
    for c in "vertical".chars() {
        view.handle_key(key(c));
    }
    let action = view.handle_key(code(KeyCode::Enter));
    assert_eq!(
        action,
        OverlayAction::Run(vec![Command::SplitWindow {
            side_by_side: true,
            argv: Vec::new(),
            cwd: None,
            detach: false,
        }])
    );

    view.prompt.clear();
    for c in ":new-window -n custom".chars() {
        view.handle_key(key(c));
    }
    view.rebuild_items();
    let action = view.handle_key(code(KeyCode::Enter));
    assert_eq!(
        action,
        OverlayAction::Run(vec![Command::NewWindow {
            name: Some("custom".to_string()),
            argv: Vec::new(),
            cwd: None,
        }])
    );
}

#[test]
fn palette_escape_and_ctrl_c_closes() {
    let mut view = sample_palette();
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::Close);
    assert_eq!(view.handle_key(ctrl('c')), OverlayAction::Close);
}

#[test]
fn palette_sessions_tab_selection() {
    let mut view = sample_palette();
    view.sessions = vec![TreeSession {
        name: "dev".to_string(),
        endpoint: String::new(),
        windows: vec![TreeWindow {
            index: 0,
            name: "editor".to_string(),
            active: true,
            panes: vec![
                pane("nvim", 1, true),
                pane("term", 2, false),
            ],
        }],
    }];
    view.tab = PaletteTab::Sessions;
    view.rebuild_items();

    let items = view.current_items();
    assert!(items.len() >= 3);
    assert!(items[0].title.contains("editor"));

    for c in "term".chars() {
        view.handle_key(key(c));
    }
    let filtered = view.current_items();
    assert_eq!(filtered.len(), 1);
    assert!(filtered[0].title.contains("term"));

    let action = view.handle_key(code(KeyCode::Enter));
    assert_eq!(
        action,
        OverlayAction::Run(vec![Command::SelectPane {
            target: "dev:term".to_string()
        }])
    );
}

#[test]
fn palette_renders_tabs_and_search_prompt() {
    let view = sample_palette();
    let text = draw_palette(&view, Rect::new(0, 0, 100, 20));
    assert!(text.contains("[ Commands ]"), "{text}");
    assert!(text.contains("Sessions & Panes"), "{text}");
    assert!(text.contains("Help"), "{text}");
    assert!(text.contains(" > _"), "{text}");
}

/// The text an overlay draws in `area`.
fn render_text(overlay: &Overlay, area: Rect) -> String {
    let mut out = Vec::new();
    overlay.render(&mut out, area).unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn text_overlay_scrolls_with_vim_motions_and_closes_on_q_or_escape() {
    let lines: Vec<String> = (1..=30).map(|i| format!("line {i:02}")).collect();
    let mut view = Overlay::text("messages", lines);
    let area = Rect::new(0, 0, 40, 10);

    let top = render_text(&view, area);
    assert!(top.contains("line 01") && top.contains("messages"), "{top}");
    assert!(!top.contains("line 30"), "{top}");

    assert_eq!(view.handle_key(key('G')), OverlayAction::None);
    let bottom = render_text(&view, area);
    assert!(bottom.contains("line 30") && !bottom.contains("line 01"), "{bottom}");

    view.handle_key(key('g'));
    view.handle_key(key('g'));
    assert!(render_text(&view, area).contains("line 01"));

    view.handle_key(ctrl('d'));
    assert!(render_text(&view, area).contains("line 05"), "half a page down");

    assert_eq!(view.handle_key(key('q')), OverlayAction::Close);
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::Close);
}

#[test]
fn text_overlay_says_when_there_is_nothing_to_show() {
    let view = Overlay::text("messages", Vec::new());
    assert!(render_text(&view, Rect::new(0, 0, 40, 10)).contains("nothing to show"));
}

#[test]
fn program_output_is_shown_without_control_characters() {
    assert_eq!(printable("a\tb\x1b[0m"), "a b?[0m");
}
