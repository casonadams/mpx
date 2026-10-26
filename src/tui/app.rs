use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::{stdout, BufWriter, Write};
use crossterm::execute;
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
    let mut copy_mode_offset: usize = 0;
    let mut split_counter: usize = 1;

    // Ensure at least one active running shell node
    let list = client.list().await?;
    let has_running = list.iter().any(|n| n.status == "running");
    if !has_running {
        let count = list.len() + 1;
        let shell = default_shell();
        let _ = client
            .spawn(format!("shell-{}", count), shell, vec![], true, None)
            .await?;
    }

    // Preload existing buffer logs into virtual screens
    if let Ok(nodes) = client.list().await {
        for node in nodes {
            if let Ok(logs) = client.logs(&node.name, None).await {
                if !logs.is_empty() {
                    let vt = vt_screens
                        .entry(node.name)
                        .or_insert_with(|| VtScreen::new(80, 24));
                    vt.feed(logs.as_bytes());
                }
            }
        }
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

    let mut out = BufWriter::with_capacity(64 * 1024, stdout());
    let mut pane_dimensions: HashMap<String, (u16, u16)> = HashMap::new();

    loop {
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
        let layout_h = term_h.saturating_sub(1); // 1 line reserved for tmux status bar

        let (raw_panes, dividers, tabs) = client.layout(term_w, layout_h).await.unwrap_or_default();
        if raw_panes.is_empty() {
            return Ok(());
        }
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

        // Handle PTY resizing and VT stream reflow whenever pane dimensions change
        for (name, rect, _) in &panes {
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            let current_dim = (rect.width, rect.height);
            let prev_dim = pane_dimensions.get(name).copied();
            if prev_dim != Some(current_dim) {
                pane_dimensions.insert(name.clone(), current_dim);
                let _ = client.resize_pane(name, rect.width, rect.height).await;

                // Replay raw stream buffer at the new column width for perfect reflow
                if let Ok(logs) = client.logs(name, None).await {
                    let mut new_vt = VtScreen::new(rect.width as usize, rect.height as usize);
                    new_vt.feed(logs.as_bytes());
                    vt_screens.insert(name.clone(), new_vt);
                }
            }
        }

        // Hide hardware cursor during frame buffer generation
        let _ = execute!(out, crossterm::cursor::Hide);

        let mut active_rect: Option<Rect> = None;

        for (name, rect, is_focused) in &panes {
            if *is_focused {
                active_rect = Some(*rect);
            }

            if rect.width > 0 && rect.height > 0 {
                let vt = vt_screens
                    .entry(name.clone())
                    .or_insert_with(|| VtScreen::new(rect.width as usize, rect.height as usize));

                let _ = Compositor::draw_pane_content(&mut out, *rect, vt.grid());
            }
        }

        // Draw tmux-style single-character dividers between panes
        if zoomed_pane.is_none() && !dividers.is_empty() {
            let _ = Compositor::draw_dividers(&mut out, &dividers);
        }

        // Clean tmux-style bottom status bar
        let _ = Compositor::draw_status_bar(
            &mut out,
            session,
            &tabs,
            &panes,
            leader_active,
            term_w,
            layout_h,
        );

        // Render modern right-aligned horizontal HUD chip when leader is active
        if leader_active {
            let _ = Compositor::draw_shortcut_hud(&mut out, term_w, layout_h);
        }

        // Render copy mode badge in top-right of active pane
        if copy_mode_offset > 0 {
            if let Some(rect) = active_rect {
                let _ = Compositor::draw_copy_mode_badge(&mut out, rect, copy_mode_offset);
            }
        }

        // Position hardware terminal cursor at active shell prompt and unhide
        if let (Some(rect), Some(vt)) = (active_rect, vt_screens.get(&active_pane)) {
            let cursor = vt.grid().cursor;
            let _ = Compositor::position_cursor(
                &mut out,
                rect,
                cursor.x,
                cursor.y,
                cursor.visible,
            );
        }

        // Atomic single flush to terminal emulator
        let _ = out.flush();

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
                        // Handle Copy Mode navigation (tmux vi-keys)
                        if copy_mode_offset > 0 {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => {
                                    copy_mode_offset = copy_mode_offset.saturating_sub(1);
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    copy_mode_offset += 1;
                                }
                                KeyCode::Char('d') if is_ctrl => {
                                    copy_mode_offset = copy_mode_offset.saturating_sub(15);
                                }
                                KeyCode::Char('u') if is_ctrl => {
                                    copy_mode_offset += 15;
                                }
                                KeyCode::PageDown => {
                                    copy_mode_offset = copy_mode_offset.saturating_sub(15);
                                }
                                KeyCode::PageUp => {
                                    copy_mode_offset += 15;
                                }
                                KeyCode::Char('g') => {
                                    copy_mode_offset = 10000;
                                }
                                KeyCode::Char('G') => {
                                    copy_mode_offset = 0;
                                }
                                KeyCode::Char('y') | KeyCode::Enter => {
                                    if let Ok(logs) = client.logs(&active_pane, None).await {
                                        super::render::copy_to_clipboard(&logs);
                                    }
                                    copy_mode_offset = 0;
                                }
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    copy_mode_offset = 0;
                                }
                                _ => {}
                            }
                            continue;
                        }


                        if leader_active {
                            leader_active = false;

                            let is_split_vert = key.code == KeyCode::Char('%')
                                || key.code == KeyCode::Char('|')
                                || key.code == KeyCode::Char('v')
                                || (key.code == KeyCode::Char('5') && key.modifiers.contains(KeyModifiers::SHIFT));

                            let is_split_horz = key.code == KeyCode::Char('"')
                                || key.code == KeyCode::Char('-')
                                || key.code == KeyCode::Char('s')
                                || key.code == KeyCode::Char('h')
                                || (key.code == KeyCode::Char('\'') && key.modifiers.contains(KeyModifiers::SHIFT));

                            if is_split_vert {
                                zoomed_pane = None;
                                split_counter += 1;
                                let new_name = format!("pane-{}", split_counter);
                                let shell = default_shell();
                                let target = if active_pane.is_empty() { None } else { Some(active_pane.clone()) };
                                let _ = client.split(target, new_name, shell, vec![], true, Direction::Horizontal).await;
                            } else if is_split_horz {
                                zoomed_pane = None;
                                split_counter += 1;
                                let new_name = format!("pane-{}", split_counter);
                                let shell = default_shell();
                                let target = if active_pane.is_empty() { None } else { Some(active_pane.clone()) };
                                let _ = client.split(target, new_name, shell, vec![], true, Direction::Vertical).await;
                            } else {
                                match key.code {
                                    // Next pane in current window (tmux o or Tab)
                                    KeyCode::Char('o') | KeyCode::Tab => {
                                        let _ = client.focus_next().await;
                                    }
                                    // Next window tab (tmux n)
                                    KeyCode::Char('n') => {
                                        zoomed_pane = None;
                                        let _ = client.next_window().await;
                                    }
                                    // Previous window tab (tmux p)
                                    KeyCode::Char('p') => {
                                        zoomed_pane = None;
                                        let _ = client.prev_window().await;
                                    }
                                    // Direct window jump by number (tmux 0..9)
                                    KeyCode::Char(d @ '0'..='9') => {
                                        zoomed_pane = None;
                                        let idx = d.to_digit(10).unwrap() as usize;
                                        let _ = client.select_window(idx).await;
                                    }
                                    // Directional pane navigation (arrows or hjkl)
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        let _ = client.focus_prev().await;
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        let _ = client.focus_next().await;
                                    }
                                    KeyCode::Left | KeyCode::Char('h') => {
                                        let _ = client.focus_prev().await;
                                    }
                                    KeyCode::Right | KeyCode::Char('l') => {
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
                                    // New window tab (tmux c)
                                    KeyCode::Char('c') => {
                                        zoomed_pane = None;
                                        let shell = default_shell();
                                        let _ = client.new_window(None, shell, vec![], true).await;
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
                                KeyCode::Char('[') => {
                                    copy_mode_offset = 1;
                                }
                                KeyCode::Esc => {
                                    // Cancel shortcut popup
                                }
                                _ => {}
                            }
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
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                for (name, rect, _) in &panes {
                                    if rect.contains(mouse.column, mouse.row) {
                                        let _ = client.focus(name).await;
                                        break;
                                    }
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                copy_mode_offset += 3;
                            }
                            MouseEventKind::ScrollDown => {
                                copy_mode_offset = copy_mode_offset.saturating_sub(3);
                            }
                            _ => {}
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
                        vt_screens.remove(&name);
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
        KeyCode::Backspace => vec![0x7F],
        KeyCode::Delete => vec![0x1B, b'[', b'3', b'~'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1B],
        KeyCode::Up => vec![0x1B, b'[', b'A'],
        KeyCode::Down => vec![0x1B, b'[', b'B'],
        KeyCode::Right => vec![0x1B, b'[', b'C'],
        KeyCode::Left => vec![0x1B, b'[', b'D'],
        KeyCode::Home => vec![0x1B, b'[', b'H'],
        KeyCode::End => vec![0x1B, b'[', b'F'],
        KeyCode::PageUp => vec![0x1B, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1B, b'[', b'6', b'~'],
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
