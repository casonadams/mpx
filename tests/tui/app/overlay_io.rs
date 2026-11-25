//! Overlay refresh and preview loading against in-process daemons sharing one socket dir.
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use mpx::client::Client;
use mpx::config::Config;
use mpx::layout::Rect;
use mpx::tui::overlay::tree_view::TreeView;
use mpx::tui::overlay::{OverlayCtx, PaletteTab, PaletteView};

use super::support::{Sandbox, pane};
use crate::tui::overlay::draw_tree;

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

async fn screen_until(client: &mut Client, name: &str, needle: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
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
    sandbox: Sandbox,
    alpha: String,
}

async fn start_fleet() -> Fleet {
    let sandbox = Sandbox::new();
    let config = Config::default();
    sandbox.serve("alpha", &config).await;
    sandbox.serve("beta", &config).await;
    let alpha = sandbox.endpoint("alpha");
    std::fs::write(sandbox.socket_dir.join("ghost.sock"), "").expect("ghost socket file");

    let mut a = Client::connect(&alpha).await.expect("connect alpha");
    a.spawn(pane("worker", "echo ready-marker; sleep 30"))
        .await
        .expect("spawn worker");
    screen_until(&mut a, "worker", "ready-marker").await;

    let mut b = Client::connect(&sandbox.endpoint("beta"))
        .await
        .expect("connect beta");
    b.spawn(pane("shell", "sleep 30"))
        .await
        .expect("spawn shell");

    Fleet { sandbox, alpha }
}

/// The fleet's tree as the alpha session sees it, after one refresh.
async fn refreshed_tree(fleet: &Fleet) -> TreeView {
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        cwd: None,
        socket_dir: fleet.sandbox.socket_dir.as_path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;
    tree
}

#[tokio::test]
async fn tree_refresh_loads_every_reachable_session_with_its_windows_and_panes() {
    let fleet = start_fleet().await;
    let tree = refreshed_tree(&fleet).await;

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
    let mut tree = refreshed_tree(&fleet).await;

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
        socket_dir: fleet.sandbox.socket_dir.as_path(),
    };
    let mut tree = TreeView::default();
    tree.refresh(&mut ctx).await;
    assert_eq!(tree.labels()[0], "alpha");

    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(text.contains("worker · "), "{text}");
    assert!(text.contains("ready-marker"), "{text}");

    tree.handle_key(key('G'));
    tree.follow_selection().await;
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(
        text.contains("shell · "),
        "the preview follows the cursor: {text}"
    );
    tree.handle_key(key('g'));
    tree.handle_key(key('g'));
    tree.follow_selection().await;

    tree.handle_key(key('p'));
    tree.refresh(&mut ctx).await;
    let text = draw_tree(&tree, Rect::new(0, 0, 120, 30));
    assert!(!text.contains("Preview"), "{text}");
    assert!(!text.contains("ready-marker"), "{text}");
}

#[cfg(unix)]
#[tokio::test]
async fn tree_refresh_skips_a_session_that_never_answers_without_waiting_on_it_for_long() {
    let fleet = start_fleet().await;
    let hung = std::os::unix::net::UnixListener::bind(fleet.sandbox.socket_dir.join("hung.sock"))
        .expect("hung daemon socket");

    let tree = tokio::time::timeout(Duration::from_secs(10), refreshed_tree(&fleet))
        .await
        .expect("a silent session must not stall the refresh");
    let labels = tree.labels();
    assert!(labels.contains(&"alpha".to_string()), "{labels:?}");
    assert!(labels.contains(&"beta".to_string()), "{labels:?}");
    assert!(!labels.contains(&"hung".to_string()), "{labels:?}");
    drop(hung);
}

#[tokio::test]
async fn palette_reloads_sessions_only_while_its_sessions_tab_is_shown() {
    let fleet = start_fleet().await;
    let config = Config::default();
    let mut client = Client::connect(&fleet.alpha).await.expect("connect");
    let mut ctx = OverlayCtx {
        client: &mut client,
        session: "alpha",
        config: &config,
        cwd: None,
        socket_dir: fleet.sandbox.socket_dir.as_path(),
    };
    let mut palette = PaletteView::new(PaletteTab::Commands, Vec::new(), "alpha");
    palette.refresh(&mut ctx).await;
    assert_eq!(palette.sessions.len(), 2);

    fleet.sandbox.serve("gamma", &config).await;
    palette.refresh(&mut ctx).await;
    assert_eq!(
        palette.sessions.len(),
        2,
        "not reloaded off the sessions tab"
    );

    palette.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(palette.tab, PaletteTab::Sessions);
    palette.refresh(&mut ctx).await;
    assert_eq!(palette.sessions.len(), 3);
}
