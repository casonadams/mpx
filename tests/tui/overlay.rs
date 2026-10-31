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

/// Runs `run` on each input and compares it with the expected output.
fn check<I, E, O>(cases: &[(I, E)], run: impl Fn(I) -> O)
where
    I: Copy + std::fmt::Debug,
    E: Copy + std::fmt::Debug,
    O: PartialEq<E> + std::fmt::Debug,
{
    for &(input, want) in cases {
        assert_eq!(run(input), want, "{input:?}");
    }
}

#[test]
fn fuzzy_match_is_case_insensitive_ordered_subsequence() {
    let cases = [
        (("", "anything"), true),
        (("clde", "Claude"), true),
        (("a-b", "alpha-beta"), true),
        (("xyz", "anything"), false),
        (("ba", "ab"), false),
    ];
    check(&cases, |(needle, haystack)| fuzzy_match(needle, haystack));
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

/// Presses `p`, collapses `label` with `h`, and draws the tree in `area`.
fn collapse_and_draw(mut view: TreeView, label: &str, area: Rect) -> String {
    view.handle_key(key('p'));
    assert_eq!(act(&mut view, label, key('h')), OverlayAction::None);
    draw_tree(&view, area)
}

/// Opens the filter with `/` and types `text`, feeding each key to `press`.
fn type_filter(mut press: impl FnMut(KeyEvent) -> OverlayAction, text: &str) {
    press(key('/'));
    for c in text.chars() {
        press(key(c));
    }
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
fn tree_h_collapses_and_l_expands_sessions_and_windows() {
    let full = tree().labels();
    let cases: [(&str, Vec<&str>); 3] = [
        ("main", vec!["main", "work", "edit", "claude"]),
        ("shell", vec!["main", "shell", "logs", "tail", "work", "edit", "claude"]),
        ("work", vec!["main", "shell", "zsh", "vim", "logs", "tail", "work"]),
    ];
    for (label, collapsed) in cases {
        let mut view = tree();
        assert_eq!(act(&mut view, label, key('h')), OverlayAction::None, "h on {label}");
        assert_eq!(view.labels(), collapsed, "h on {label}");
        assert_eq!(act(&mut view, label, key('l')), OverlayAction::None, "l on {label} must not select");
        assert_eq!(view.labels(), full, "l on {label} expands it");
    }
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
fn tree_enter_acts_on_the_row_it_is_on() {
    let cases: [(&str, OverlayAction); 5] = [
        ("main", OverlayAction::Close),
        (
            "work",
            OverlayAction::Run(vec![Command::SwitchSession {
                name: "work".to_string(),
            }]),
        ),
        ("logs", OverlayAction::Run(vec![Command::SelectWindow { index: 1 }])),
        (
            "edit",
            OverlayAction::Run(vec![
                Command::SwitchSession {
                    name: "work".to_string(),
                },
                Command::SelectWindow { index: 0 },
            ]),
        ),
        (
            "claude",
            OverlayAction::Run(vec![Command::SelectPane {
                target: "work:%4".to_string(),
            }]),
        ),
    ];
    for (label, want) in cases {
        let mut view = tree();
        assert_eq!(act(&mut view, label, code(KeyCode::Enter)), want, "Enter on {label}");
    }
}

#[test]
fn tree_x_asks_to_kill_the_row_it_is_on() {
    let cases: [(&str, OverlayAction); 4] = [
        (
            "claude",
            OverlayAction::Run(vec![Command::Confirm {
                prompt: "kill pane claude?".to_string(),
                then: Box::new(Command::KillPane {
                    target: Some("work:%4".to_string()),
                }),
            }]),
        ),
        (
            "edit",
            OverlayAction::Run(vec![
                Command::SwitchSession {
                    name: "work".to_string(),
                },
                Command::Confirm {
                    prompt: "kill window edit?".to_string(),
                    then: Box::new(Command::KillWindow { index: Some(0) }),
                },
            ]),
        ),
        (
            "work",
            OverlayAction::Run(vec![
                Command::SwitchSession {
                    name: "work".to_string(),
                },
                Command::Confirm {
                    prompt: "kill session work?".to_string(),
                    then: Box::new(Command::KillServer),
                },
            ]),
        ),
        (
            "main",
            OverlayAction::Run(vec![Command::Confirm {
                prompt: "kill session main?".to_string(),
                then: Box::new(Command::KillServer),
            }]),
        ),
    ];
    for (label, want) in cases {
        let mut view = tree();
        assert_eq!(act(&mut view, label, key('x')), want, "x on {label}");
    }
}

#[test]
fn tree_filter_keeps_matching_rows_and_their_ancestors() {
    let cases = [
        (tree(), "vim", vec!["main", "shell", "vim"]),
        // Matches a pane's program; its session and window stay as ancestors.
        (busy_tree(), "claude", vec!["work", "editor", "claude"]),
    ];
    for (mut view, query, want) in cases {
        type_filter(|k| view.handle_key(k), query);
        assert_eq!(view.labels(), want, "filter {query:?}");
    }
}

#[test]
fn overlays_close_on_their_close_keys() {
    let cases: [(&str, Overlay, KeyEvent); 6] = [
        ("tree q", Overlay::Tree(tree()), key('q')),
        ("tree esc", Overlay::Tree(tree()), code(KeyCode::Esc)),
        ("help q", Overlay::help(help_entries()), key('q')),
        ("help esc", Overlay::help(help_entries()), code(KeyCode::Esc)),
        ("palette esc", Overlay::palette(sample_palette()), code(KeyCode::Esc)),
        ("palette ctrl-c", Overlay::palette(sample_palette()), ctrl('c')),
    ];
    for (name, mut overlay, k) in cases {
        assert_eq!(overlay.handle_key(k), OverlayAction::Close, "{name}");
    }
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
    type_filter(|k| help.handle_key(k), "close");
    assert_eq!(help.visible().len(), 1);
    assert_eq!(help.visible()[0].key, "q");
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

#[test]
fn overlays_render_into_tiny_and_empty_areas_without_panicking() {
    for area in [
        Rect::new(0, 0, 0, 0),
        Rect::new(0, 0, 1, 1),
        Rect::new(0, 0, 5, 2),
    ] {
        render_text(&Overlay::tree(), area);
        render_text(&Overlay::help(help_entries()), area);
    }
}

#[test]
fn overlays_render_their_title_and_entries() {
    let cases: [(&str, Overlay, Rect, &[&str]); 2] = [
        ("tree", Overlay::Tree(tree()), Rect::new(0, 0, 60, 20), &["main", "zsh"]),
        (
            "help",
            Overlay::help(help_entries()),
            Rect::new(0, 0, 60, 10),
            &["Help", "close overlay"],
        ),
    ];
    for (name, overlay, area, needles) in cases {
        let text = render_text(&overlay, area);
        for needle in needles {
            assert!(text.contains(*needle), "{name} should show {needle:?}:\n{text}");
        }
    }
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

pub(super) fn draw_tree(view: &TreeView, area: Rect) -> String {
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
    let text = collapse_and_draw(tree(), "logs", Rect::new(0, 0, 80, 30));
    assert_row(&text, "logs", "▸ 1 logs");
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

fn assert_row(text: &str, row: &str, needle: &str) {
    assert!(row_with(text, row).contains(needle), "{needle:?} not on row {row:?}:\n{text}");
}

#[test]
fn tree_lists_the_current_session_first() {
    assert_eq!(busy_tree().labels()[0], "main");
}

/// `busy_tree` drawn at the size the busy-tree render tests share.
fn busy_text() -> String {
    draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12))
}

#[test]
fn tree_title_counts_sessions() {
    let text = busy_text();
    assert!(text.contains("Sessions · 2"), "{text}");
}

#[test]
fn busy_tree_rows_show_current_marker_program_and_exit_code() {
    let text = busy_text();
    assert_row(&text, "▾ main", "current");
    assert_row(&text, "zsh", "zsh");
    assert_row(&text, "build", "exit 1");
    assert!(!row_with(&text, "▾ work").contains("current"), "{text}");
}

#[test]
fn tree_collapsed_session_rolls_up_exits() {
    let text = collapse_and_draw(busy_tree(), "main", Rect::new(0, 0, 100, 12));
    assert_row(&text, "main", "1 exited");
}

#[test]
fn tree_narrow_rows_drop_the_detail_column() {
    let mut view = busy_tree();
    view.handle_key(key('p'));
    let text = draw_tree(&view, Rect::new(0, 0, 60, 12));
    assert!(!text.contains("panes"), "{text}");
}

#[test]
fn tree_preview_names_the_pane_a_session_row_would_show() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 20));
    assert_row(&text, "main:%3", "zsh");
}

#[test]
fn short_path_replaces_only_a_whole_home_prefix() {
    let cases = [
        ("/home/me", "~"),
        ("/home/me/src", "~/src"),
        ("/home/other", "/home/other"),
        ("/opt", "/opt"),
    ];
    check(&cases, |path| short_path(path, Some("/home/me")));
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
    for needle in ["[ Commands ]", "Sessions & Panes", "Help", " > _"] {
        assert!(text.contains(needle), "{needle:?} missing:\n{text}");
    }
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
