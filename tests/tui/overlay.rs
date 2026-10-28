use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mpx::agents::{LiveAgent, ResumableSession};
use mpx::layout::{Rect, WindowTab};
use mpx::protocol::{AgentInfo, AgentState, NodeInfo, NodeState};
use mpx::tui::command::Command;
use mpx::tui::overlay::nav::fuzzy_match;
use mpx::tui::overlay::tree_view::session_from_snapshot;
use mpx::tui::overlay::widgets::{age_label, short_path};
use mpx::tui::overlay::{
    AgentsView, HelpEntry, HelpView, Overlay, OverlayAction, TreePane, TreeSession, TreeView,
    TreeWindow,
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

fn type_text(view: &mut AgentsView, text: &str) {
    for c in text.chars() {
        view.handle_key(key(c));
    }
}

fn agent(
    session: &str,
    name: &str,
    id: u32,
    state: Option<AgentState>,
    since_ms: u64,
    output_ms: u64,
) -> LiveAgent {
    let mut node = NodeInfo::new(name.to_string(), true, "claude".to_string());
    node.id = id;
    node.last_output_ms = output_ms;
    node.agent = state.map(|state| AgentInfo {
        harness: "claude".to_string(),
        state,
        session_id: Some(format!("sid-{id}")),
        detail: None,
        since_ms,
        focus: Default::default(),
    });
    LiveAgent {
        session: session.to_string(),
        endpoint: String::new(),
        node,
    }
}

fn exited(mut live: LiveAgent) -> LiveAgent {
    live.node.state = NodeState::Exited;
    live
}

fn with_harness(mut live: LiveAgent, harness: &str) -> LiveAgent {
    if let Some(info) = live.node.agent.as_mut() {
        info.harness = harness.to_string();
    }
    live
}

fn resume(harness: &str, id: &str, title: &str, cwd: Option<&str>) -> ResumableSession {
    ResumableSession {
        harness: harness.to_string(),
        id: id.to_string(),
        title: title.to_string(),
        cwd: cwd.map(str::to_string),
        modified_ms: 0,
        path: PathBuf::new(),
    }
}

/// alpha(NeedsInput, since 200), beta(NeedsInput, since 100), gamma/delta(Working), eps(Idle), dead(Exited).
fn fleet() -> AgentsView {
    AgentsView::from_data(
        vec![
            agent("s", "alpha", 1, Some(AgentState::NeedsInput), 200, 0),
            agent("s", "beta", 2, Some(AgentState::NeedsInput), 100, 0),
            agent("s", "gamma", 3, Some(AgentState::Working), 0, 500),
            agent("s", "delta", 4, Some(AgentState::Working), 0, 900),
            agent("s", "eps", 5, Some(AgentState::Idle), 0, 0),
            exited(agent("s", "dead", 6, Some(AgentState::Working), 0, 0)),
        ],
        Vec::new(),
        vec!["claude".to_string(), "omp".to_string()],
        Some("/work".to_string()),
    )
}

fn names(view: &AgentsView) -> Vec<String> {
    view.visible_names()
}

#[test]
fn agents_groups_follow_state_order_and_sort_within_groups() {
    let view = fleet();
    assert_eq!(
        view.group_titles(),
        vec!["NEEDS YOU", "WORKING", "IDLE", "EXITED"]
    );
    assert_eq!(
        names(&view),
        vec!["beta", "alpha", "delta", "gamma", "eps", "dead"]
    );
}

#[test]
fn agents_resume_section_is_titled_with_scope_and_listed_last() {
    let view = AgentsView::from_data(
        vec![agent("s", "alpha", 1, Some(AgentState::Idle), 0, 0)],
        vec![
            resume("claude", "r1", "fix bug", Some("/work")),
            resume("omp", "r2", "notes", None),
        ],
        Vec::new(),
        Some("/work".to_string()),
    );
    assert_eq!(view.group_titles(), vec!["IDLE", "RESUME · /work"]);
    assert_eq!(names(&view), vec!["alpha", "r1", "r2"]);
}

#[test]
fn agents_resume_title_says_all_when_scope_is_toggled() {
    let mut view = AgentsView::from_data(
        Vec::new(),
        vec![resume("claude", "r1", "fix bug", Some("/work"))],
        Vec::new(),
        Some("/work".to_string()),
    );
    assert!(!view.scope_all());
    assert_eq!(view.group_titles(), vec!["RESUME · /work"]);
    assert_eq!(view.handle_key(key('s')), OverlayAction::None);
    assert!(view.scope_all());
    assert_eq!(view.group_titles(), vec!["RESUME · all"]);
    view.handle_key(key('s'));
    assert!(!view.scope_all());
}

#[test]
fn fuzzy_match_is_case_insensitive_ordered_subsequence() {
    assert!(fuzzy_match("", "anything"));
    assert!(fuzzy_match("clde", "Claude"));
    assert!(fuzzy_match("a-b", "alpha-beta"));
    assert!(!fuzzy_match("tb", "alpha-beta"));
    assert!(!fuzzy_match("xyz", "alpha"));
}

#[test]
fn agents_filter_narrows_live_rows_and_enter_keeps_it() {
    let mut view = fleet();
    view.handle_key(key('/'));
    type_text(&mut view, "dl");
    assert_eq!(names(&view), vec!["delta"]);
    assert_eq!(view.handle_key(code(KeyCode::Enter)), OverlayAction::None);
    assert_eq!(names(&view), vec!["delta"]);
    assert_eq!(view.handle_key(key('j')), OverlayAction::None);
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::None);
    assert_eq!(names(&view).len(), 6);
}

#[test]
fn agents_filter_matches_harness_and_backspace_widens_again() {
    let mut view = AgentsView::from_data(
        vec![
            with_harness(agent("s", "one", 1, Some(AgentState::Idle), 0, 0), "omp"),
            agent("s", "two", 2, Some(AgentState::Idle), 0, 0),
        ],
        Vec::new(),
        Vec::new(),
        None,
    );
    view.handle_key(key('/'));
    type_text(&mut view, "omp");
    assert_eq!(names(&view), vec!["one"]);
    view.handle_key(code(KeyCode::Backspace));
    view.handle_key(code(KeyCode::Backspace));
    view.handle_key(code(KeyCode::Backspace));
    assert_eq!(names(&view), vec!["one", "two"]);
}

#[test]
fn agents_esc_in_filter_clears_text_then_closes() {
    let mut view = fleet();
    view.handle_key(key('/'));
    type_text(&mut view, "zzz");
    assert!(names(&view).is_empty());
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::None);
    assert_eq!(names(&view).len(), 6);
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::Close);
    assert_eq!(view.handle_key(key('q')), OverlayAction::Close);
}

#[test]
fn agents_enter_on_live_selects_its_pane_in_its_session() {
    let mut view = fleet();
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%2".to_string()
        }])
    );
}

#[test]
fn agents_enter_on_resumable_launches_with_resume_id_and_its_cwd() {
    let mut view = AgentsView::from_data(
        Vec::new(),
        vec![resume("omp", "abc", "notes", Some("/proj"))],
        Vec::new(),
        None,
    );
    let launch = Command::LaunchAgent {
        harness: "omp".to_string(),
        resume: Some("abc".to_string()),
        cwd: Some("/proj".to_string()),
        split: false,
    };
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![launch.clone()])
    );
    assert_eq!(view.handle_key(key('r')), OverlayAction::Run(vec![launch]));
}

#[test]
fn agents_capital_r_resumes_as_split() {
    let mut view = AgentsView::from_data(
        Vec::new(),
        vec![resume("claude", "abc", "notes", Some("/proj"))],
        Vec::new(),
        None,
    );
    assert_eq!(
        view.handle_key(key('R')),
        OverlayAction::Run(vec![Command::LaunchAgent {
            harness: "claude".to_string(),
            resume: Some("abc".to_string()),
            cwd: Some("/proj".to_string()),
            split: true,
        }])
    );
}

#[test]
fn agents_resume_keys_do_nothing_on_live_rows() {
    let mut view = fleet();
    assert_eq!(view.handle_key(key('r')), OverlayAction::None);
    assert_eq!(view.handle_key(key('R')), OverlayAction::None);
}

#[test]
fn agents_new_agent_picks_harness_then_launches_in_ctx_cwd() {
    let mut view = fleet();
    view.handle_key(key('n'));
    view.handle_key(key('j'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::LaunchAgent {
            harness: "omp".to_string(),
            resume: None,
            cwd: Some("/work".to_string()),
            split: false,
        }])
    );
}

#[test]
fn agents_harness_picker_escape_cancels_without_launching() {
    let mut view = fleet();
    view.handle_key(key('n'));
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::None);
    assert_eq!(view.handle_key(key('q')), OverlayAction::Close);
}

#[test]
fn agents_message_goes_to_every_marked_agent_in_order() {
    let mut view = fleet();
    view.handle_key(code(KeyCode::Tab));
    view.handle_key(key('j'));
    view.handle_key(code(KeyCode::Tab));
    view.handle_key(key('m'));
    type_text(&mut view, "hi");
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![
            Command::Paste {
                target: Some("s:%1".to_string()),
                text: "hi".to_string(),
                submit: true,
            },
            Command::Paste {
                target: Some("s:%2".to_string()),
                text: "hi".to_string(),
                submit: true,
            },
        ])
    );
}

#[test]
fn agents_message_without_marks_goes_to_selected_agent_only() {
    let mut view = fleet();
    view.handle_key(key('m'));
    type_text(&mut view, "ping");
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::Paste {
            target: Some("s:%2".to_string()),
            text: "ping".to_string(),
            submit: true,
        }])
    );
}

#[test]
fn agents_message_escape_discards_input_and_empty_message_sends_nothing() {
    let mut view = fleet();
    view.handle_key(key('m'));
    type_text(&mut view, "draft");
    assert_eq!(view.handle_key(code(KeyCode::Esc)), OverlayAction::None);
    assert_eq!(view.handle_key(key('q')), OverlayAction::Close);

    view.handle_key(key('m'));
    assert_eq!(view.handle_key(code(KeyCode::Enter)), OverlayAction::None);
}

#[test]
fn agents_message_on_resumable_row_has_no_target() {
    let mut view = AgentsView::from_data(
        Vec::new(),
        vec![resume("claude", "abc", "notes", None)],
        Vec::new(),
        None,
    );
    view.handle_key(key('m'));
    type_text(&mut view, "hello");
    assert_eq!(view.handle_key(code(KeyCode::Enter)), OverlayAction::None);
}

#[test]
fn agents_tab_toggles_mark_on_and_off() {
    let mut view = fleet();
    view.handle_key(code(KeyCode::Tab));
    view.handle_key(code(KeyCode::Tab));
    view.handle_key(key('m'));
    type_text(&mut view, "x");
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::Paste {
            target: Some("s:%2".to_string()),
            text: "x".to_string(),
            submit: true,
        }])
    );
}

#[test]
fn agents_kill_confirms_then_kills_the_pane() {
    let mut view = fleet();
    assert_eq!(
        view.handle_key(key('x')),
        OverlayAction::Run(vec![Command::Confirm {
            prompt: "kill beta?".to_string(),
            then: Box::new(Command::KillPane {
                target: Some("s:%2".to_string()),
            }),
        }])
    );
}

#[test]
fn agents_yank_copies_live_session_id_or_resume_id() {
    let view = fleet();
    assert_eq!(view.yank_text(), Some("sid-2".to_string()));
    let resumable = AgentsView::from_data(
        Vec::new(),
        vec![resume("claude", "abc", "notes", None)],
        Vec::new(),
        None,
    );
    assert_eq!(resumable.yank_text(), Some("abc".to_string()));
}

#[test]
fn agents_gg_and_capital_g_jump_to_bounds() {
    let mut view = fleet();
    view.handle_key(key('G'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%6".to_string()
        }])
    );
    view.handle_key(key('g'));
    view.handle_key(key('g'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%2".to_string()
        }])
    );
}

#[test]
fn agents_k_at_top_and_j_at_bottom_stay_in_bounds() {
    let mut view = fleet();
    view.handle_key(key('k'));
    view.handle_key(code(KeyCode::Up));
    assert_eq!(names(&view)[0], "beta");
    view.handle_key(key('G'));
    view.handle_key(key('j'));
    view.handle_key(code(KeyCode::Down));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%6".to_string()
        }])
    );
}

#[test]
fn agents_ctrl_d_and_ctrl_u_move_half_page_and_clamp() {
    let mut view = fleet();
    view.handle_key(ctrl('d'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%6".to_string()
        }])
    );
    view.handle_key(ctrl('u'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%2".to_string()
        }])
    );
}

#[test]
fn agents_g_then_other_key_cancels_the_pending_g() {
    let mut view = fleet();
    view.handle_key(key('G'));
    view.handle_key(key('g'));
    view.handle_key(key('k'));
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectPane {
            target: "s:%5".to_string()
        }])
    );
}

#[test]
fn agents_preview_is_on_by_default() {
    let text = render_to_string(
        &Overlay::Agents(AgentsView::default()),
        Rect::new(0, 0, 120, 20),
    );
    assert!(text.contains("Preview"));
}

#[test]
fn agents_preview_panel_shows_when_wide_and_hides_on_p() {
    let mut view = fleet();
    let wide = render_to_string(&Overlay::Agents(fleet()), Rect::new(0, 0, 120, 20));
    assert!(wide.contains("Preview"));
    assert_eq!(view.handle_key(key('p')), OverlayAction::None);
    let hidden = render_to_string(&Overlay::Agents(view), Rect::new(0, 0, 120, 20));
    assert!(!hidden.contains("Preview"));
    let short = render_to_string(&Overlay::Agents(fleet()), Rect::new(0, 0, 120, 11));
    assert!(!short.contains("Preview"));
}

#[test]
fn age_labels_use_largest_whole_unit() {
    assert_eq!(age_label(12_000), "12s");
    assert_eq!(age_label(59_999), "59s");
    assert_eq!(age_label(3 * 60_000), "3m");
    assert_eq!(age_label(2 * 3_600_000), "2h");
    assert_eq!(age_label(5 * 86_400_000), "5d");
}

fn pane(name: &str, id: u32, agent: Option<AgentState>, active: bool) -> TreePane {
    TreePane {
        name: name.to_string(),
        id,
        agent,
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
                        panes: vec![
                            pane("zsh", 1, None, true),
                            pane("claude", 2, Some(AgentState::Idle), false),
                        ],
                    },
                    TreeWindow {
                        index: 1,
                        name: "logs".to_string(),
                        active: false,
                        panes: vec![pane("tail", 3, None, false)],
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
                    panes: vec![pane("vim", 9, None, true)],
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
        .expect("label visible");
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
            "main", "shell", "zsh", "claude", "logs", "tail", "work", "edit", "vim"
        ]
    );
}

#[test]
fn tree_h_collapses_a_session_and_l_expands_it_again() {
    let mut view = tree();
    assert_eq!(act(&mut view, "main", key('h')), OverlayAction::None);
    assert_eq!(view.labels(), vec!["main", "work", "edit", "vim"]);
    assert_eq!(act(&mut view, "main", key('l')), OverlayAction::None);
    assert_eq!(view.labels().len(), 9);
}

#[test]
fn tree_h_on_a_leaf_moves_to_its_parent_window() {
    let mut view = tree();
    assert_eq!(act(&mut view, "zsh", key('h')), OverlayAction::None);
    assert_eq!(
        view.handle_key(code(KeyCode::Enter)),
        OverlayAction::Run(vec![Command::SelectWindow { index: 0 }])
    );
}

#[test]
fn tree_collapsed_window_hides_its_panes() {
    let mut view = tree();
    assert_eq!(act(&mut view, "shell", key('h')), OverlayAction::None);
    assert_eq!(
        view.labels(),
        vec!["main", "shell", "logs", "tail", "work", "edit", "vim"]
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
            target: "main:%2".to_string()
        }])
    );
}

#[test]
fn tree_l_on_collapsed_session_expands_instead_of_selecting() {
    let mut view = tree();
    act(&mut view, "work", key('h'));
    assert_eq!(view.labels().len(), 7);
    assert_eq!(act(&mut view, "work", key('l')), OverlayAction::None);
    assert_eq!(view.labels().len(), 9);
}

#[test]
fn tree_x_confirms_kill_for_each_node_kind() {
    let mut view = tree();
    assert_eq!(
        act(&mut view, "claude", key('x')),
        OverlayAction::Run(vec![Command::Confirm {
            prompt: "kill pane claude?".to_string(),
            then: Box::new(Command::KillPane {
                target: Some("main:%2".to_string()),
            }),
        }])
    );
    assert_eq!(
        act(&mut view, "logs", key('x')),
        OverlayAction::Run(vec![Command::Confirm {
            prompt: "kill window logs?".to_string(),
            then: Box::new(Command::KillWindow { index: Some(1) }),
        }])
    );
    assert_eq!(
        act(&mut view, "work", key('x')),
        OverlayAction::Run(vec![
            Command::SwitchSession {
                name: "work".to_string()
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
    assert_eq!(view.labels(), vec!["work", "edit", "vim"]);
    view.handle_key(code(KeyCode::Esc));
    assert_eq!(view.labels().len(), 9);
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
    let nodes = vec![
        node("a", 1, Some(0)),
        node("b", 2, Some(1)),
        node("c", 3, None),
    ];
    let tabs = vec![
        WindowTab {
            index: 0,
            name: "w0".to_string(),
            is_active: true,
        },
        WindowTab {
            index: 1,
            name: "w1".to_string(),
            is_active: false,
        },
    ];
    let session = session_from_snapshot("main".to_string(), &nodes, &tabs, Some("a"));
    assert_eq!(session.windows.len(), 2);
    assert_eq!(session.windows[0].panes.len(), 1);
    assert!(session.windows[0].panes[0].active);
    assert_eq!(session.windows[1].panes[0].name, "b");
    assert!(!session.windows[1].panes[0].active);
}

#[test]
fn session_snapshot_prefers_foreground_program_and_keeps_exit_status() {
    let mut shell = NodeInfo::new("a".to_string(), true, "/bin/zsh -l".to_string());
    shell.window = Some(0);
    shell.cwd = Some("/tmp".to_string());
    let mut editor = shell.clone();
    editor.name = "b".to_string();
    editor.foreground = Some("nvim".to_string());
    editor.state = NodeState::Exited;
    editor.exit_code = Some(1);
    let tabs = vec![WindowTab {
        index: 0,
        name: "w0".to_string(),
        is_active: true,
    }];
    let session = session_from_snapshot("main".to_string(), &[shell, editor], &tabs, None);
    let panes = &session.windows[0].panes;
    assert_eq!(panes[0].program, "zsh");
    assert_eq!(panes[0].cwd.as_deref(), Some("/tmp"));
    assert!(panes[0].running);
    assert_eq!(panes[1].program, "nvim");
    assert!(!panes[1].running);
    assert_eq!(panes[1].exit_code, Some(1));
}

fn help_entries() -> Vec<HelpEntry> {
    [
        ("j/k", "move selection"),
        ("q", "close overlay"),
        ("C-b a", "open agents"),
    ]
    .into_iter()
    .map(|(k, c)| HelpEntry {
        key: k.to_string(),
        command: c.to_string(),
    })
    .collect()
}

#[test]
fn help_filter_matches_key_or_command_text() {
    let mut help = HelpView::new(help_entries());
    help.handle_key(key('/'));
    for c in "agent".chars() {
        help.handle_key(key(c));
    }
    let shown: Vec<String> = help.visible().iter().map(|e| e.key.clone()).collect();
    assert_eq!(shown, vec!["C-b a"]);
    assert_eq!(help.handle_key(code(KeyCode::Enter)), OverlayAction::None);
    assert_eq!(help.handle_key(code(KeyCode::Esc)), OverlayAction::None);
    assert_eq!(help.visible().len(), 3);
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
        key('G'),
        key('g'),
        key('g'),
        ctrl('d'),
        ctrl('u'),
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
fn agents_render_shows_header_groups_and_rows() {
    let overlay = Overlay::Agents(fleet());
    let text = render_to_string(&overlay, Rect::new(0, 0, 120, 20));
    assert!(text.contains("Agents · 2 need you"));
    assert!(text.contains("NEEDS YOU"));
    assert!(text.contains("beta"));
    assert!(text.contains("WORKING"));
}

#[test]
fn overlays_render_into_tiny_and_empty_areas_without_panicking() {
    for area in [
        Rect::new(0, 0, 0, 0),
        Rect::new(0, 0, 1, 1),
        Rect::new(0, 0, 3, 2),
    ] {
        render_to_string(&Overlay::agents(), area);
        render_to_string(&Overlay::tree(), area);
        render_to_string(&Overlay::help(help_entries()), area);
    }
}

#[test]
fn tree_render_shows_session_and_pane_names() {
    let text = render_to_string(&Overlay::Tree(tree()), Rect::new(0, 0, 60, 20));
    assert!(text.contains("main"));
    assert!(text.contains("claude"));
    assert!(text.contains("Sessions · 2"));
}

#[test]
fn help_render_shows_entries_and_title() {
    let text = render_to_string(&Overlay::help(help_entries()), Rect::new(0, 0, 60, 10));
    assert!(text.contains("Help"));
    assert!(text.contains("close overlay"));
}

fn draw_agents(view: &AgentsView, area: Rect) -> String {
    let mut buf: Vec<u8> = Vec::new();
    view.render(&mut buf, area).expect("render");
    String::from_utf8_lossy(&buf).into_owned()
}

/// Screen text after drawing the tree, so assertions ignore styling escapes.
fn draw_tree(view: &TreeView, area: Rect) -> String {
    let mut buf: Vec<u8> = Vec::new();
    view.render(&mut buf, area).expect("render");
    let mut screen = mpx::vt::VtScreen::new(
        usize::from(area.x + area.width),
        usize::from(area.y + area.height),
    );
    screen.feed(&buf);
    screen.snapshot()
}

#[test]
fn agents_render_groups_appear_in_state_order_under_their_headers() {
    let mut view = fleet();
    view.handle_key(key('p'));
    let text = draw_agents(&view, Rect::new(0, 0, 120, 30));
    let at: Vec<usize> = ["NEEDS YOU", "WORKING", "IDLE", "EXITED"]
        .iter()
        .map(|title| {
            text.find(title)
                .unwrap_or_else(|| panic!("{title} missing"))
        })
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
    assert!(text.contains("◆ NEEDS YOU"));
    assert!(text.contains("Agents · 2 need you"));
}

#[test]
fn agents_render_rows_show_state_glyph_and_mark() {
    let mut view = fleet();
    view.handle_key(code(KeyCode::Tab));
    view.handle_key(key('p'));
    let text = draw_agents(&view, Rect::new(0, 0, 120, 30));
    assert!(text.contains("+◆ "), "marked needs-input row");
    assert!(text.contains("◆ NEEDS YOU"), "header glyph");
    assert!(text.contains("● "), "working row");
    assert!(text.contains("○ "), "idle row");
    assert!(text.contains("✕ "), "exited row");
    assert!(text.contains("eps"));
    assert!(text.contains("dead"));
}

#[test]
fn agents_render_preview_of_live_agent_without_screen_is_titled() {
    let text = draw_agents(&fleet(), Rect::new(0, 0, 120, 20));
    // Needs-input rows sort oldest first, so `beta` is selected.
    assert!(text.contains("beta"));
    assert!(!text.contains("Preview · alpha") && text.contains("Preview"));
}

#[test]
fn agents_render_has_no_preview_pane_when_narrow_or_toggled_off() {
    let short = draw_agents(&fleet(), Rect::new(0, 0, 120, 11));
    assert!(!short.contains("Preview"));
    let mut view = fleet();
    view.handle_key(key('p'));
    let toggled = draw_agents(&view, Rect::new(0, 0, 120, 20));
    assert!(!toggled.contains("Preview"));
}

#[test]
fn tree_render_shows_expanded_and_collapsed_arrows() {
    let mut view = tree();
    view.handle_key(key('p'));
    let expanded = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(expanded.contains("▾ main"), "{expanded}");
    assert!(expanded.contains("▾ 0 shell"), "{expanded}");
    assert!(expanded.contains("▾ work"), "{expanded}");
    assert_eq!(act(&mut view, "work", key('h')), OverlayAction::None);
    let collapsed = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(collapsed.contains("▸ work"), "{collapsed}");
    assert!(!collapsed.contains("▾ work"), "{collapsed}");
    assert!(!collapsed.contains("vim"), "{collapsed}");
}

#[test]
fn tree_render_shows_collapsed_window_arrow_and_hides_its_panes() {
    let mut view = tree();
    view.handle_key(key('p'));
    assert_eq!(act(&mut view, "logs", key('h')), OverlayAction::None);
    let text = draw_tree(&view, Rect::new(0, 0, 80, 30));
    assert!(text.contains("▸ 1 logs"), "{text}");
    assert!(!text.contains("tail"), "{text}");
    assert!(text.contains("▾ 0 shell"), "{text}");
}

#[test]
fn help_view_renders_its_entries_directly() {
    let view = HelpView::new(help_entries());
    let mut buf: Vec<u8> = Vec::new();
    view.render(&mut buf, Rect::new(0, 0, 60, 10))
        .expect("render");
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("close overlay"), "{text}");
}

#[test]
fn agents_title_shows_selection_position_and_preview_meta_row() {
    let text = draw_agents(&fleet(), Rect::new(0, 0, 120, 20));
    assert!(text.contains("Agents · 2 need you · 1/"), "{text}");
}

#[test]
fn agents_preview_yields_its_rows_to_the_list_while_filtering() {
    let mut view = fleet();
    assert!(draw_agents(&view, Rect::new(0, 0, 120, 20)).contains("Preview"));
    view.handle_key(key('/'));
    assert!(!draw_agents(&view, Rect::new(0, 0, 120, 20)).contains("Preview"));
}

/// `main` (current, listed second) has an agent waiting, an exited build and a pane under home.
fn busy_tree() -> TreeView {
    let mut waiting = pane("claude", 4, Some(AgentState::NeedsInput), false);
    waiting.cwd = Some("/home/me/src/mpx".to_string());
    let mut build = pane("build", 5, None, false);
    build.running = false;
    build.exit_code = Some(2);
    let mut shell = pane("zsh", 3, None, true);
    shell.program = "nvim".to_string();
    TreeView::from_data(
        vec![
            TreeSession {
                name: "work".to_string(),
                endpoint: String::new(),
                windows: vec![TreeWindow {
                    index: 0,
                    name: "edit".to_string(),
                    active: true,
                    panes: vec![pane("vim", 9, None, true)],
                }],
            },
            TreeSession {
                name: "main".to_string(),
                endpoint: String::new(),
                windows: vec![TreeWindow {
                    index: 0,
                    name: "dev".to_string(),
                    active: true,
                    panes: vec![shell, waiting, build],
                }],
            },
        ],
        "main",
    )
    .with_home(Some("/home/me"))
}

/// Screen row containing `needle`.
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
fn tree_title_counts_sessions_and_agents_waiting() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(text.contains("Sessions · 2 · 1 need you"), "{text}");
    let quiet = draw_tree(&tree(), Rect::new(0, 0, 100, 12));
    assert!(!quiet.contains("need you"), "{quiet}");
}

#[test]
fn tree_marks_current_session_active_window_and_focused_pane_distinctly() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(row_with(&text, "▾ main").contains("current"), "{text}");
    assert!(!row_with(&text, "▾ work").contains("current"), "{text}");
    assert!(row_with(&text, "0 dev").contains("active"), "{text}");
    assert!(row_with(&text, "zsh").contains('›'), "{text}");
    assert!(!row_with(&text, "claude").contains('›'), "{text}");
}

#[test]
fn tree_pane_rows_show_program_short_cwd_state_and_exit_code() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 12));
    assert!(row_with(&text, "zsh").contains("nvim"), "{text}");
    let claude = row_with(&text, "claude");
    assert!(claude.contains('◆') && claude.contains("~/src/mpx"), "{text}");
    let build = row_with(&text, "build");
    assert!(build.contains('✕') && build.contains("exit 2"), "{text}");
}

#[test]
fn tree_collapsed_session_rolls_up_waiting_agents_and_exits() {
    let mut view = busy_tree();
    view.handle_key(key('p'));
    assert_eq!(act(&mut view, "main", key('h')), OverlayAction::None);
    let text = draw_tree(&view, Rect::new(0, 0, 100, 12));
    let main = row_with(&text, "▸ main");
    assert!(main.contains("◆1") && main.contains("✕1"), "{text}");
    assert!(main.contains("1 window · 3 panes"), "{text}");
    assert!(!text.contains("claude"), "{text}");
}

#[test]
fn tree_narrow_rows_drop_the_detail_column() {
    let mut view = busy_tree();
    view.handle_key(key('p'));
    let text = draw_tree(&view, Rect::new(0, 0, 60, 12));
    assert!(!text.contains("3 panes"), "{text}");
    assert!(row_with(&text, "build").contains("exit 2"), "{text}");
}

#[test]
fn tree_filter_matches_pane_program_and_directory() {
    let mut view = busy_tree();
    view.handle_key(key('/'));
    "nvim".chars().for_each(|c| {
        view.handle_key(key(c));
    });
    assert_eq!(view.labels(), vec!["main", "dev", "zsh"]);
    view.handle_key(code(KeyCode::Esc));
    view.handle_key(key('/'));
    "src/mpx".chars().for_each(|c| {
        view.handle_key(key(c));
    });
    assert_eq!(view.labels(), vec!["main", "dev", "claude"]);
}

#[test]
fn tree_preview_names_the_pane_a_session_row_would_show() {
    let text = draw_tree(&busy_tree(), Rect::new(0, 0, 100, 20));
    let meta = row_with(&text, "main:%3");
    assert!(meta.contains("zsh") && meta.contains("nvim"), "{text}");
    assert!(text.contains("nothing to preview yet"), "{text}");
}

#[test]
fn short_path_replaces_only_a_whole_home_prefix() {
    let home = Some("/home/me");
    assert_eq!(short_path("/home/me", home), "~");
    assert_eq!(short_path("/home/me/src", home), "~/src");
    assert_eq!(short_path("/home/meow", home), "/home/meow");
    assert_eq!(short_path("/tmp", home), "/tmp");
    assert_eq!(short_path("/home/me/src", None), "/home/me/src");
}
