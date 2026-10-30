//! Overlay refresh and preview loading against in-process daemons sharing one socket dir.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use mpx::client::Client;
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::{IpcListener, endpoint_in};
use mpx::layout::Rect;
use mpx::protocol::SpawnOpts;
use mpx::tui::overlay::tree_view::TreeView;
use mpx::tui::overlay::OverlayCtx;

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
        dir.join(format!("{session}-state")),
    )
    .await;
    tokio::spawn(async move {
        loop {
            if let Ok(stream) = listener.accept().await {
                tokio::spawn(handle_client(Arc::clone(&state), stream));
            }
        }
    });
    endpoint
}

fn pty(name: &str, command: &str) -> SpawnOpts {
    SpawnOpts {
        name: name.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), command.to_string()],
        pty: true,
        ..SpawnOpts::default()
    }
}

async fn screen_until(client: &mut Client, name: &str, needle: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(screen) = client.screen(name).await
            && screen.contains(needle)
        {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("screen of '{name}' never contained '{needle}'");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

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
    screen_until(&mut a, "worker", "ready-marker").await;

    let mut b = Client::connect(&beta).await.expect("connect beta");
    b.spawn(pty("shell", "sleep 30"))
        .await
        .expect("spawn shell");

    Fleet { dir, alpha }
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

#[tokio::test]
async fn tree_refresh_loads_every_reachable_session_with_its_windows_and_panes() {
    let fleet = start_fleet().await;
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
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
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
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

#[tokio::test]
async fn tree_preview_shows_the_selected_sessions_focused_pane_and_toggles_off() {
    let fleet = start_fleet().await;
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        cwd: None,
        socket_dir: fleet.dir.path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;
    assert_eq!(tree.labels()[0], "alpha");

    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("alpha:%"), "{text}");
    assert!(text.contains("ready-marker"), "{text}");

    tree.handle_key(key('G'));
    tree.follow_selection().await;
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("beta:%"), "the preview follows the cursor: {text}");
    tree.handle_key(key('g'));
    tree.handle_key(key('g'));
    tree.follow_selection().await;

    tree.handle_key(key('p'));
    tree.refresh(&mut ctx).await;
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(!text.contains("Preview"), "{text}");
    assert!(!text.contains("ready-marker"), "{text}");
}
