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
        // Redraw current workspace
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
        let layout_h = term_h.saturating_sub(1); // 1 line reserved for status bar

        let panes = client.layout(term_w, layout_h).await.unwrap_or_default();
        let active_pane = panes
            .iter()
            .find(|(.., focused)| *focused)
            .map(|(name, ..)| name.clone())
            .unwrap_or_default();

        for (name, rect, is_focused) in &panes {
            let _ = Compositor::draw_borders(&mut stdout, *rect, name, *is_focused);

            if rect.width > 2 && rect.height > 2 {
                let inner = Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2);
                let vt = vt_screens
                    .entry(name.clone())
                    .or_insert_with(|| VtScreen::new(inner.width as usize, inner.height as usize));
                let _ = Compositor::draw_pane_content(&mut stdout, inner, vt.grid());
            }
        }

        let _ = Compositor::draw_status_bar(
            &mut stdout,
            session,
            &active_pane,
            leader_active,
            term_w,
            layout_h,
        );

        tokio::select! {
            Some(Ok(event)) = event_reader.next() => {
                match event {
                    Event::Key(key) => {
                        // Check for Ctrl-A leader key
                        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
                            leader_active = !leader_active;
                            continue;
                        }

                        if leader_active {
                            leader_active = false;
                            match key.code {
                                KeyCode::Char('|') | KeyCode::Char('v') => {
                                    let count = vt_screens.len() + 1;
                                    let shell = default_shell();
                                    let _ = client.split(None, format!("pane-{}", count), shell, vec![], true, Direction::Vertical).await;
                                }
                                KeyCode::Char('-') | KeyCode::Char('h') => {
                                    let count = vt_screens.len() + 1;
                                    let shell = default_shell();
                                    let _ = client.split(None, format!("pane-{}", count), shell, vec![], true, Direction::Horizontal).await;
                                }
                                KeyCode::Char('o') | KeyCode::Char('n') | KeyCode::Tab => {
                                    let _ = client.focus_next().await;
                                }
                                KeyCode::Char('p') => {
                                    let _ = client.focus_prev().await;
                                }
                                KeyCode::Char('x') => {
                                    if !active_pane.is_empty() {
                                        let _ = client.kill(&active_pane).await;
                                    }
                                }
                                KeyCode::Char('d') => {
                                    // Detach from TUI
                                    return Ok(());
                                }
                                _ => {}
                            }
                        } else {
                            // Forward key input to active pane
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
