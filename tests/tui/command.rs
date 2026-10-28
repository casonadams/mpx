use mpx::protocol::{LayoutPreset, Side};
use mpx::tui::command::{Command, parse, split_words};

fn ok(line: &str) -> Command {
    parse(line).unwrap_or_else(|e| panic!("`{line}` should parse: {e}"))
}

fn err(line: &str) -> String {
    parse(line).expect_err("should fail")
}

#[test]
fn split_window_default_is_stacked_and_vsplit_is_side_by_side() {
    assert!(matches!(
        ok("split-window"),
        Command::SplitWindow {
            side_by_side: false,
            ..
        }
    ));
    assert!(matches!(
        ok("split-window -h"),
        Command::SplitWindow {
            side_by_side: true,
            ..
        }
    ));
    assert!(matches!(
        ok("split"),
        Command::SplitWindow {
            side_by_side: false,
            ..
        }
    ));
    assert!(matches!(
        ok("sp"),
        Command::SplitWindow {
            side_by_side: false,
            ..
        }
    ));
    assert!(matches!(
        ok("vsplit"),
        Command::SplitWindow {
            side_by_side: true,
            ..
        }
    ));
    assert!(matches!(
        ok("vs"),
        Command::SplitWindow {
            side_by_side: true,
            ..
        }
    ));
}

#[test]
fn split_window_takes_cwd_detach_and_command() {
    let cmd = ok("split-window -d -c /tmp/x htop -d");
    assert_eq!(
        cmd,
        Command::SplitWindow {
            side_by_side: false,
            argv: vec!["htop".into(), "-d".into()],
            cwd: Some("/tmp/x".into()),
            detach: true,
        }
    );
}

#[test]
fn new_window_and_aliases() {
    assert_eq!(
        ok("new-window -n editor -c /w vim a.txt"),
        Command::NewWindow {
            name: Some("editor".into()),
            argv: vec!["vim".into(), "a.txt".into()],
            cwd: Some("/w".into()),
        }
    );
    for alias in ["new", "tabnew"] {
        assert!(
            matches!(ok(alias), Command::NewWindow { name: None, .. }),
            "{alias}"
        );
    }
}

#[test]
fn window_navigation_names_and_aliases() {
    assert_eq!(ok("next-window"), Command::NextWindow);
    assert_eq!(ok("tabnext"), Command::NextWindow);
    assert_eq!(ok("tabn"), Command::NextWindow);
    assert_eq!(ok("previous-window"), Command::PrevWindow);
    assert_eq!(ok("tabprevious"), Command::PrevWindow);
    assert_eq!(ok("tabp"), Command::PrevWindow);
    assert_eq!(ok("last-window"), Command::LastWindow);
    assert_eq!(ok("select-window -t 3"), Command::SelectWindow { index: 3 });
    assert_eq!(
        ok("select-window -t :2"),
        Command::SelectWindow { index: 2 }
    );
}

#[test]
fn kill_commands_take_optional_targets() {
    assert_eq!(ok("kill-pane"), Command::KillPane { target: None });
    assert_eq!(
        ok("kill-pane -t api"),
        Command::KillPane {
            target: Some("api".into())
        }
    );
    assert_eq!(ok("kill-window"), Command::KillWindow { index: None });
    assert_eq!(
        ok("kill-window -t 1"),
        Command::KillWindow { index: Some(1) }
    );
}

#[test]
fn kill_server_is_wrapped_in_confirm() {
    match ok("qa") {
        Command::Confirm { then, .. } => assert_eq!(*then, Command::KillServer),
        other => panic!("expected Confirm, got {other:?}"),
    }
    assert!(matches!(ok("kill-server"), Command::Confirm { .. }));
}

#[test]
fn rename_window_joins_words() {
    assert_eq!(
        ok("rename-window api server"),
        Command::RenameWindow {
            name: "api server".into()
        }
    );
    assert!(err("rename-window   ").contains("missing name"));
}

#[test]
fn select_pane_by_target_or_direction() {
    assert_eq!(
        ok("select-pane -t build"),
        Command::SelectPane {
            target: "build".into()
        }
    );
    assert_eq!(
        ok("select-pane -L"),
        Command::SelectPaneDir { side: Side::Left }
    );
    assert_eq!(
        ok("select-pane -R"),
        Command::SelectPaneDir { side: Side::Right }
    );
    assert_eq!(
        ok("select-pane -U"),
        Command::SelectPaneDir { side: Side::Up }
    );
    assert_eq!(
        ok("select-pane -D"),
        Command::SelectPaneDir { side: Side::Down }
    );
    assert!(err("select-pane").contains("need -t"));
}

#[test]
fn resize_pane_defaults_to_five_cells_and_supports_zoom() {
    assert_eq!(
        ok("resize-pane -L"),
        Command::ResizePane {
            side: Side::Left,
            cells: 5
        }
    );
    assert_eq!(
        ok("resize-pane -D 12"),
        Command::ResizePane {
            side: Side::Down,
            cells: 12
        }
    );
    assert_eq!(ok("resize-pane -Z"), Command::Zoom);
    assert!(err("resize-pane").contains("need -L/-R/-U/-D or -Z"));
    assert!(err("resize-pane -L many").contains("invalid cell count"));
}

#[test]
fn swap_pane_directions() {
    assert_eq!(ok("swap-pane -U"), Command::SwapPane { next: false });
    assert_eq!(ok("swap-pane -D"), Command::SwapPane { next: true });
    assert!(err("swap-pane").contains("need -U or -D"));
}

#[test]
fn layouts_by_name_or_cycle() {
    assert_eq!(
        ok("select-layout tiled"),
        Command::SelectLayout {
            preset: Some(LayoutPreset::Tiled)
        }
    );
    assert_eq!(ok("select-layout"), Command::SelectLayout { preset: None });
    assert_eq!(ok("next-layout"), Command::SelectLayout { preset: None });
    assert!(err("select-layout diagonal").contains("unknown layout 'diagonal'"));
}

#[test]
fn sessions_and_aliases() {
    assert_eq!(
        ok("switch-client -t work"),
        Command::SwitchSession {
            name: "work".into()
        }
    );
    assert_eq!(
        ok("session work"),
        Command::SwitchSession {
            name: "work".into()
        }
    );
    assert_eq!(ok("next-session"), Command::NextSession);
    assert_eq!(ok("prev-session"), Command::PrevSession);
    assert!(err("switch-client").contains("need -t <session>"));
}

#[test]
fn send_keys_and_paste() {
    assert_eq!(
        ok("send-keys -t web Enter C-c"),
        Command::SendKeys {
            target: Some("web".into()),
            keys: vec!["Enter".into(), "C-c".into()]
        }
    );
    assert_eq!(
        ok("paste --submit 'two words'"),
        Command::Paste {
            target: None,
            text: "two words".into(),
            submit: true
        }
    );
    assert!(err("send-keys").contains("no keys"));
    assert!(err("paste").contains("missing text"));
}

#[test]
fn detach_quit_and_prompts() {
    for name in ["detach-client", "detach", "q", "quit"] {
        assert_eq!(ok(name), Command::Detach, "{name}");
    }
    assert_eq!(ok("copy-mode"), Command::CopyMode);
    assert_eq!(
        ok("command-prompt -I 'rename-window '"),
        Command::CommandPrompt {
            initial: "rename-window ".into()
        }
    );
    assert_eq!(ok("list-keys"), Command::OpenHelp);
    assert_eq!(ok("choose-tree"), Command::OpenTree);
    assert_eq!(ok("agents"), Command::OpenAgents);
    assert_eq!(ok("next-attention"), Command::NextAttention);
}

#[test]
fn launch_agent_options() {
    assert_eq!(
        ok("launch-agent claude --resume abc --cwd /repo --split"),
        Command::LaunchAgent {
            harness: "claude".into(),
            resume: Some("abc".into()),
            cwd: Some("/repo".into()),
            split: true,
        }
    );
    assert!(err("launch-agent").contains("missing harness"));
}

#[test]
fn unknown_command_and_unknown_option_are_reported() {
    assert_eq!(err("frobnicate"), "unknown command: frobnicate");
    assert!(err("split-window -x").contains("unknown option -x"));
    assert!(err("kill-pane extra").contains("unexpected argument 'extra'"));
    assert!(err("   ").contains("empty command"));
}

#[test]
fn split_words_honours_quotes_and_escapes() {
    assert_eq!(
        split_words("a 'b c' \"d e\"").unwrap(),
        vec!["a", "b c", "d e"]
    );
    assert_eq!(
        split_words(r#""say \"hi\"" x"#).unwrap(),
        vec![r#"say "hi""#, "x"]
    );
    assert_eq!(split_words(r"a\ b").unwrap(), vec!["a b"]);
    assert_eq!(split_words("''").unwrap(), vec![""]);
    assert_eq!(
        split_words("'keep \\ slash'").unwrap(),
        vec!["keep \\ slash"]
    );
}

#[test]
fn unterminated_quote_is_an_error() {
    assert_eq!(split_words("echo 'oops").unwrap_err(), "unterminated quote");
    assert_eq!(err("paste \"open"), "unterminated quote");
}
