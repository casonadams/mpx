//! Overlay refresh and preview loading against in-process daemons sharing one socket dir.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use mpx::agents::{Harness, ResumableSession};
use mpx::client::{AgentReportArgs, Client};
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::{IpcListener, endpoint_in};
use mpx::layout::Rect;
use mpx::protocol::{AgentState, SpawnOpts};
use mpx::tui::command::Command;
use mpx::tui::overlay::tree_view::TreeView;
use mpx::tui::overlay::{AgentsView, OverlayAction, OverlayCtx};

fn area() -> Rect {
    Rect::new(0, 0, 120, 20)
}

fn code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key(c: char) -> KeyEvent {
    code(KeyCode::Char(c))
}

async fn start_daemon(dir: &Path, session: &str) -> String {
    let endpoint = endpoint_in(dir, session);
    #[allow(unused_mut)]
    let mut listener = IpcListener::bind(&endpoint).expect("bind");
    let state = DaemonState::new(
        session.to_string(),
        endpoint.clone(),
        Config::default(),
        dir.join(format!("state-{session}")),
    )
    .await;
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            tokio::spawn(handle_client(Arc::clone(&state), stream));
        }
    });
    endpoint
}

fn pty(name: &str, script: &str) -> SpawnOpts {
    SpawnOpts {
        name: name.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        pty: true,
        ..SpawnOpts::default()
    }
}

/// Waits (bounded) until the pane's screen shows `needle`.
async fn screen_until(client: &mut Client, name: &str, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let screen = client.screen(name).await.expect("screen");
        if screen.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "'{needle}' never reached the screen"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Two sessions, `alpha` with a Working agent pane `worker` printing a marker and `beta` with a
/// plain shell pane. The socket dir also holds a dead `ghost` socket that must be skipped.
struct Fleet {
    dir: TempDir,
    alpha: String,
}

async fn start_fleet() -> Fleet {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = start_daemon(dir.path(), "alpha").await;
    let beta = start_daemon(dir.path(), "beta").await;
    std::fs::write(dir.path().join("ghost.sock"), "").expect("ghost socket file");

    let mut a = Client::connect(&alpha).await.expect("connect alpha");
    a.spawn(pty("worker", "echo ready-marker; sleep 30"))
        .await
        .expect("spawn worker");
    a.agent_report(
        "worker",
        AgentReportArgs {
            harness: Some("claude".to_string()),
            state: Some(AgentState::Working),
            session_id: Some("sid-1".to_string()),
            detail: Some("refactoring".to_string()),
            ..AgentReportArgs::default()
        },
    )
    .await
    .expect("report agent");
    screen_until(&mut a, "worker", "ready-marker").await;

    let mut b = Client::connect(&beta).await.expect("connect beta");
    b.spawn(pty("shell", "sleep 30"))
        .await
        .expect("spawn shell");

    Fleet { dir, alpha }
}

fn harness_registry() -> Vec<Harness> {
    vec![Harness {
        name: "claude".to_string(),
        detect: Vec::new(),
        command: vec!["claude".to_string()],
        resume: Vec::new(),
        session_id_args: Vec::new(),
        store: None,
    }]
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

fn transcript(dir: &Path, lines: &str) -> ResumableSession {
    let path: PathBuf = dir.join("chat.jsonl");
    std::fs::write(&path, lines).expect("write transcript");
    ResumableSession {
        harness: "claude".to_string(),
        id: "sid-9".to_string(),
        title: "chat".to_string(),
        cwd: Some("/work".to_string()),
        modified_ms: 0,
        path,
    }
}

#[tokio::test]
async fn agents_refresh_lists_live_agent_with_its_session_and_loads_its_screen() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: Some("/work"),
        socket_dir: fleet.dir.path(),
    };
    let mut view = AgentsView::default();
    view.refresh(&mut ctx).await;

    assert_eq!(view.visible_names(), vec!["worker".to_string()]);
    let text = draw_agents(&view, area());
    assert!(text.contains("WORKING"), "{text}");
    assert!(text.contains("refactoring"), "{text}");
    assert!(text.contains("alpha:%"), "{text}");
    assert!(text.contains("worker"), "{text}");
    assert!(text.contains("ready-marker"), "{text}");

    let opened = view.handle_key(code(KeyCode::Enter));
    assert!(
        matches!(&opened, OverlayAction::Run(cmds)
            if matches!(cmds.as_slice(), [Command::SelectPane { target }] if target.starts_with("alpha:%"))),
        "{opened:?}"
    );
}

#[tokio::test]
async fn agents_refresh_offers_registry_harnesses_and_current_cwd_for_new_agents() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: Some("/work"),
        socket_dir: fleet.dir.path(),
    };
    let mut view = AgentsView::default();
    view.refresh(&mut ctx).await;

    view.handle_key(key('n'));
    let launched = view.handle_key(code(KeyCode::Enter));
    assert_eq!(
        launched,
        OverlayAction::Run(vec![Command::LaunchAgent {
            harness: "claude".to_string(),
            resume: None,
            cwd: Some("/work".to_string()),
            split: false,
        }])
    );
}

#[tokio::test]
async fn agents_preview_toggled_off_is_not_loaded_on_refresh() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: None,
        socket_dir: fleet.dir.path(),
    };
    let mut view = AgentsView::default();
    view.handle_key(key('p'));
    view.refresh(&mut ctx).await;

    let text = draw_agents(&view, area());
    assert!(text.contains("worker"), "{text}");
    assert!(!text.contains("ready-marker"), "{text}");
}

#[tokio::test]
async fn tree_refresh_loads_every_reachable_session_with_its_windows_and_panes() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: None,
        socket_dir: fleet.dir.path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;

    let labels = tree.labels();
    assert!(labels.contains(&"alpha".to_string()), "{labels:?}");
    assert!(labels.contains(&"beta".to_string()), "{labels:?}");
    assert!(labels.contains(&"worker".to_string()), "{labels:?}");
    assert!(labels.contains(&"shell".to_string()), "{labels:?}");
    assert!(!labels.contains(&"ghost".to_string()), "{labels:?}");

    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("Sessions · 2"), "{text}");
    assert!(text.contains("current"), "{text}");
    assert!(text.contains("▾ beta"), "{text}");
}

#[tokio::test]
async fn tree_refresh_then_collapsing_a_session_hides_its_panes() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: None,
        socket_dir: fleet.dir.path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;

    let beta = tree
        .labels()
        .iter()
        .position(|l| l == "beta")
        .expect("beta listed");
    for _ in 0..beta {
        tree.handle_key(key('j'));
    }
    tree.handle_key(key('h'));
    let labels = tree.labels();
    assert!(!labels.contains(&"shell".to_string()), "{labels:?}");
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("▸ beta"), "{text}");
}

#[test]
fn transcript_preview_shows_last_messages_of_stored_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = transcript(
        dir.path(),
        concat!(
            "{\"message\":{\"role\":\"user\",\"content\":\"how do I resume\"}}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"use the picker\"}]}}\n",
        ),
    );
    let view = AgentsView::from_data(Vec::new(), vec![session], Vec::new(), None);
    let text = draw_agents(&view, area());
    assert!(text.contains("chat"), "{text}");
    assert!(text.contains("you › how do I resume"), "{text}");
    assert!(text.contains(" › use the picker"), "{text}");
}

#[test]
fn transcript_preview_keeps_only_the_lines_that_fit_the_preview_pane() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = transcript(
        dir.path(),
        concat!(
            "{\"message\":{\"role\":\"user\",\"content\":\"first question\"}}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"first answer\"}}\n",
            "{\"message\":{\"role\":\"user\",\"content\":\"second question\"}}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"second answer\"}}\n",
        ),
    );
    let view = AgentsView::from_data(Vec::new(), vec![session], Vec::new(), None);
    // A 15-row overlay leaves a meta row and five body rows: one answer, a question and an answer.
    let text = draw_agents(&view, Rect::new(0, 0, 120, 15));
    assert!(!text.contains("first question"), "{text}");
    assert!(text.contains(" › first answer"), "{text}");
    assert!(text.contains("you › second question"), "{text}");
    assert!(text.contains(" › second answer"), "{text}");
}

#[test]
fn transcript_preview_of_missing_file_is_empty_and_does_not_fail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = ResumableSession {
        harness: "claude".to_string(),
        id: "sid-0".to_string(),
        title: "gone".to_string(),
        cwd: None,
        modified_ms: 0,
        path: dir.path().join("missing.jsonl"),
    };
    let view = AgentsView::from_data(Vec::new(), vec![session], Vec::new(), None);
    let text = draw_agents(&view, area());
    assert!(text.contains("nothing to preview yet"), "{text}");
    assert!(!text.contains(" › "), "{text}");
}

#[tokio::test]
async fn tree_preview_shows_the_selected_sessions_focused_pane_and_toggles_off() {
    let fleet = start_fleet().await;
    let registry = harness_registry();
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        registry: &registry,
        cwd: None,
        socket_dir: fleet.dir.path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;
    assert_eq!(tree.labels()[0], "alpha");

    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("alpha:%"), "{text}");
    assert!(text.contains("ready-marker"), "{text}");

    tree.handle_key(key('p'));
    tree.refresh(&mut ctx).await;
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(!text.contains("Preview"), "{text}");
    assert!(!text.contains("ready-marker"), "{text}");
}
