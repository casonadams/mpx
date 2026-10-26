use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::stdout;
use std::time::Duration;
use tokio::time::sleep;

use super::render::Compositor;
use super::terminal::TerminalGuard;
use crate::client::Client;
use crate::layout::geometry::Rect;
use crate::layout::Direction;
use crate::protocol::StreamEvent;
use crate::vt::VtScreen;

pub async fn run_tui(endpoint: &str, session: &str) -> Result<(), String> {
    let _guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {}", e))?;

    let mut client = Client::connect(endpoint).await?;
    let mut sub_client = Client::connect(endpoint).await?;

    let mut event_reader = EventStream::new();
    let mut leader_active = false;
    let mut zoomed_pane: Option<String> = None;
    let mut vt_screens: HashMap<String, VtScreen> = HashMap::new();

    // Spawn default shell pane if session is empty
    let list = client.list().await?;
    if list.is_empty() {
        let shell = default_shell();
        let _ = client
            .spawn("shell".into(), shell, vec![], true, None)
            .await?;
    }

    // Subscribe to event stream in background task
    let (tx_events, mut rx_events) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    tokio::spawn(async move {
        let _ = sub_client
            .stream_logs(None, |event| {
                let _ = tx_events.send(event);
                true
            })
            .await;
    });

    let mut stdout = stdout();

    loop {
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
        let layout_h = term_h.saturating_sub(1); // 1 line reserved for tmux status bar

        let raw_panes = client.layout(term_w, layout_h).await.unwrap_or_default();
        let active_pane = raw_panes
            .iter()
            .find(|(.., focused)| *focused)
            .map(|(name, ..)| name.clone())
            .unwrap_or_default();

        // Handle Zoom (tmux Leader + z)
        let panes: Vec<(String, Rect, bool)> = if let Some(z_name) = &zoomed_pane {
            vec![(
                z_name.clone(),
                Rect::new(0, 0, term_w, layout_h),
                true,
            )]
        } else {
            raw_panes.clone()
        };

        let mut active_rect: Option<Rect> = None;

        for (name, rect, is_focused) in &panes {
            if *is_focused {
                active_rect = Some(*rect);
            }

            if rect.width > 0 && rect.height > 0 {
                let vt = vt_screens
                    .entry(name.clone())
                    .or_insert_with(|| VtScreen::new(rect.width as usize, rect.height as usize));

                if vt.grid().cols != rect.width as usize || vt.grid().rows != rect.height as usize {
                    vt.grid_mut().resize(rect.width as usize, rect.height as usize);
                }

                let _ = Compositor::draw_pane_content(&mut stdout, *rect, vt.grid());
            }

            // Draw thin divider line if multiple panes are present
            if panes.len() > 1 && zoomed_pane.is_none() {
                let _ = Compositor::draw_dividers(
                    &mut stdout,
                    *rect,
                    term_w,
                    layout_h,
                    *is_focused,
                );
            }
        }

        // Clean tmux-style bottom status bar
        let _ = Compositor::draw_status_bar(
            &mut stdout,
            session,
            &panes,
            leader_active,
            term_w,
            layout_h,
        );

        // Render which-key popup when leader key is active
        if leader_active {
            let _ = Compositor::draw_which_key(&mut stdout, term_w, layout_h);
        }

        // Position hardware terminal cursor at active shell prompt
        if let (Some(rect), Some(vt)) = (active_rect, vt_screens.get(&active_pane)) {
            let cursor = vt.grid().cursor;
            let _ = Compositor::position_cursor(
                &mut stdout,
                rect,
                cursor.x,
                cursor.y,
                cursor.visible,
            );
        }

        tokio::select! {
            Some(Ok(event)) = event_reader.next() => {
                match event {
                    Event::Key(key) => {
                        // Supports both Ctrl-b (standard tmux) and Ctrl-a (common tmux rebind)
                        let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                        let is_leader_key = is_ctrl && (key.code == KeyCode::Char('b') || key.code == KeyCode::Char('a'));

                        if is_leader_key {
                            leader_active = !leader_active;
                            continue;
                        }

                        if leader_active {
                            leader_active = false;
                            match key.code {
                                // Split vertical (side-by-side columns: tmux %, |, or v)
                                KeyCode::Char('%') | KeyCode::Char('|') | KeyCode::Char('v') => {
                                    zoomed_pane = None;
                                    let count = vt_screens.len() + 1;
                                    let shell = default_shell();
                                    let _ = client.split(None, format!("pane-{}", count), shell, vec![], true, Direction::Vertical).await;
                                }
                                // Split horizontal (top/bottom rows: tmux ", -, s, or h)
                                KeyCode::Char('"') | KeyCode::Char('-') | KeyCode::Char('s') | KeyCode::Char('h') => {
                                    zoomed_pane = None;
                                    let count = vt_screens.len() + 1;
                                    let shell = default_shell();
                                    let _ = client.split(None, format!("pane-{}", count), shell, vec![], true, Direction::Horizontal).await;
                                }
                                // Next pane (tmux o, n, or Tab)
                                KeyCode::Char('o') | KeyCode::Char('n') | KeyCode::Tab => {
                                    let _ = client.focus_next().await;
                                }
                                // Previous pane (tmux p)
                                KeyCode::Char('p') => {
                                    let _ = client.focus_prev().await;
                                }
                                // Directional navigation (arrows or hjkl)
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let _ = client.focus_prev().await;
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let _ = client.focus_next().await;
                                }
                                KeyCode::Left => {
                                    let _ = client.focus_prev().await;
                                }
                                KeyCode::Right => {
                                    let _ = client.focus_next().await;
                                }
                                // Zoom toggle (tmux Leader + z)
                                KeyCode::Char('z') => {
                                    if zoomed_pane.is_some() {
                                        zoomed_pane = None;
                                    } else if !active_pane.is_empty() {
                                        zoomed_pane = Some(active_pane.clone());
                                    }
                                }
                                // New window/pane (tmux c)
                                KeyCode::Char('c') => {
                                    let count = vt_screens.len() + 1;
                                    let shell = default_shell();
                                    let _ = client.spawn(format!("pane-{}", count), shell, vec![], true, None).await;
                                }
                                // Kill pane (tmux x)
                                KeyCode::Char('x') => {
                                    if !active_pane.is_empty() {
                                        zoomed_pane = None;
                                        let _ = client.kill(&active_pane).await;
                                    }
                                }
                                // Detach (tmux d)
                                KeyCode::Char('d') => {
                                    return Ok(());
                                }
                                KeyCode::Esc => {
                                    // Cancel which-key popup
                                }
                                _ => {}
                            }
                        } else {
                            // Forward key input directly to active pane
                            if !active_pane.is_empty() {
                                let payload = key_to_bytes(key.code, key.modifiers);
                                if !payload.is_empty() {
                                    let text = String::from_utf8_lossy(&payload).into_owned();
                                    let _ = client.send_input(&active_pane, &text).await;
                                }
                            }
                        }
                    }
                    Event::Resize(..) => {}
                    _ => {}
                }
            }

            Some(stream_event) = rx_events.recv() => {
                match stream_event {
                    StreamEvent::Output { name, data } => {
                        let vt = vt_screens
                            .entry(name)
                            .or_insert_with(|| VtScreen::new(80, 24));
                        vt.feed(data.as_bytes());
                    }
                    StreamEvent::Exit { name, .. } => {
                        if let Some(vt) = vt_screens.get_mut(&name) {
                            vt.feed(b"\r\n[Process exited]\r\n");
                        }
                    }
                    _ => {}
                }
            }

            else => {
                sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

fn key_to_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = code {
            let ascii = c.to_ascii_lowercase() as u8;
            if (b'a'..=b'z').contains(&ascii) {
                return vec![ascii - b'a' + 1];
            }
        }
    }

    match code {
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x08],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1B],
        KeyCode::Up => vec![0x1B, b'[', b'A'],
        KeyCode::Down => vec![0x1B, b'[', b'B'],
        KeyCode::Right => vec![0x1B, b'[', b'C'],
        KeyCode::Left => vec![0x1B, b'[', b'D'],
        _ => Vec::new(),
    }
}

fn default_shell() -> String {
    if let Ok(sh) = std::env::var("SHELL") {
        if !sh.is_empty() {
            return sh;
        }
    }
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        "/bin/sh".to_string()
    }
}
