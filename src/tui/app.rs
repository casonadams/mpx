use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::{stdout, BufWriter, Write};
use std::time::Duration;

use super::render::{Compositor, SelectionRange};
use super::terminal::TerminalGuard;
use crate::client::Client;
use crate::layout::geometry::Rect;
use crate::layout::{Direction, Divider, WindowTab};
use crate::protocol::{NewWindowOpts, SplitOpts, StreamEvent};
use crate::vt::VtScreen;

pub async fn run_tui(endpoint: &str, session: &str) -> Result<(), String> {
    let _guard = TerminalGuard::enter().map_err(|e| format!("Failed to enter raw mode: {}", e))?;

    let mut client = Client::connect(endpoint).await?;
    let mut sub_client = Client::connect(endpoint).await?;

    let mut event_reader = EventStream::new();
    let mut leader_active = false;
    let mut zoomed_pane: Option<String> = None;
    let mut vt_screens: HashMap<String, VtScreen> = HashMap::new();
    let mut copy_state: Option<CopyState> = None;
    let mut split_counter: usize = 1;

    // Ensure at least one active window running default shell
    let list = client.list().await?;
    let has_running = list.iter().any(|n| n.status == "running");
    if !has_running {
        let shell = default_shell();
        let cur_dir = std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned());
        let _ = client
            .new_window(NewWindowOpts {
                name: None,
                command: shell,
                args: vec![],
                pty: true,
                cwd: cur_dir,
            })
            .await;
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
    let mut last_stream_render: Option<std::time::Instant> = None;
    let frame_budget = Duration::from_millis(16);
    let mut layout_cache = LayoutCache::new();
    let mut tick_timer = tokio::time::interval(Duration::from_secs(1));
    loop {
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
        let layout_h = term_h.saturating_sub(1); // 1 line reserved for tmux status bar

        if !layout_cache.sync(&mut client, term_w, layout_h).await {
            return Ok(());
        }
        let raw_panes = &layout_cache.panes;
        let dividers = &layout_cache.dividers;
        let tabs = &layout_cache.tabs;

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

        sync_pane_dimensions(&mut client, &panes, &mut pane_dimensions, &mut vt_screens).await;

        // Begin DEC 2026 synchronized update to eliminate tearing and cursor jitter
        let _ = Compositor::begin_sync(&mut out);
        let mut active_rect: Option<Rect> = None;
        for (name, rect, is_focused) in &panes {
            if *is_focused {
                active_rect = Some(*rect);
            }

            if rect.width > 0 && rect.height > 0 {
                let vt = vt_screens
                    .entry(name.clone())
                    .or_insert_with(|| VtScreen::new(rect.width as usize, rect.height as usize));

                let (sel, offset, badge) = if *is_focused {
                    let s = copy_state.as_ref().and_then(|cs| cs.selection_range(vt.grid()));
                    let off = copy_state.as_ref().map(|cs| cs.scroll_offset).unwrap_or(0);
                    let b = copy_state.as_ref().map(|cs| {
                        let label = match cs.mode {
                            SelectionKind::None => "COPY",
                            SelectionKind::Character => "VISUAL",
                            SelectionKind::Line => "VISUAL LINE",
                        };
                        let hist_len = vt.grid().history_len();
                        format!("[{} -{}/{} | y: yank | q: exit]", label, cs.scroll_offset, hist_len)
                    });
                    (s, off, b)
                } else {
                    (None, 0, None)
                };
                let _ = Compositor::draw_pane_content(
                    &mut out,
                    super::render::RenderPaneContext {
                        rect: *rect,
                        grid: vt.grid(),
                        selection: sel.as_ref(),
                        scroll_offset: offset,
                        badge: badge.as_deref(),
                        dimmed: is_dimmed(panes.len(), zoomed_pane.is_some(), *is_focused),
                    },
                );
            }
        }

        // Draw tmux-style single-character dividers between panes
        if zoomed_pane.is_none() && !dividers.is_empty() {
            let _ = Compositor::draw_dividers(&mut out, &dividers, active_rect);
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


        // Position hardware terminal cursor at active shell prompt and unhide
        if let (Some(rect), Some(vt)) = (active_rect, vt_screens.get(&active_pane)) {
            let (cx, cy, vis) = if let Some(cs) = &copy_state {
                (cs.cursor_x, cs.cursor_y, true)
            } else {
                let cursor = vt.grid().cursor;
                (cursor.x, cursor.y, cursor.visible)
            };
            let _ = Compositor::position_cursor(
                &mut out,
                rect,
                cx,
                cy,
                vis,
            );
        }
        // End DEC 2026 synchronized update and flush frame atomically
        let _ = Compositor::end_sync(&mut out);
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
                        // Handle Copy / Visual Mode navigation (tmux vi-keys)
                        if let Some(cs) = &mut copy_state {
                            let pane_w = active_rect.map(|r| r.width as usize).unwrap_or(80);
                            let pane_h = active_rect.map(|r| r.height as usize).unwrap_or(24);

                            let hist_len = vt_screens.get(&active_pane).map(|vt| vt.grid().history_len()).unwrap_or(0);

                            match key.code {
                                KeyCode::Char('h') | KeyCode::Left => {
                                    cs.cursor_x = cs.cursor_x.saturating_sub(1);
                                }
                                KeyCode::Char('l') | KeyCode::Right => {
                                    cs.cursor_x = (cs.cursor_x + 1).min(pane_w.saturating_sub(1));
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    if cs.cursor_y > 0 {
                                        cs.cursor_y -= 1;
                                    } else if cs.scroll_offset < hist_len {
                                        cs.scroll_offset += 1;
                                    }
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    if cs.cursor_y + 1 < pane_h {
                                        cs.cursor_y += 1;
                                    } else if cs.scroll_offset > 0 {
                                        cs.scroll_offset -= 1;
                                    }
                                }
                                KeyCode::Char('0') | KeyCode::Char('^') => {
                                    cs.cursor_x = 0;
                                }
                                KeyCode::Char('$') => {
                                    cs.cursor_x = pane_w.saturating_sub(1);
                                }
                                KeyCode::Char('w') => {
                                    cs.cursor_x = (cs.cursor_x + 4).min(pane_w.saturating_sub(1));
                                }
                                KeyCode::Char('b') => {
                                    cs.cursor_x = cs.cursor_x.saturating_sub(4);
                                }
                                KeyCode::Char('u') if is_ctrl => {
                                    cs.scroll_offset = (cs.scroll_offset + 15).min(hist_len);
                                }
                                KeyCode::Char('d') if is_ctrl => {
                                    cs.scroll_offset = cs.scroll_offset.saturating_sub(15);
                                }
                                KeyCode::PageUp => {
                                    cs.scroll_offset = (cs.scroll_offset + 15).min(hist_len);
                                }
                                KeyCode::PageDown => {
                                    cs.scroll_offset = cs.scroll_offset.saturating_sub(15);
                                }
                                KeyCode::Char('g') => {
                                    cs.scroll_offset = hist_len;
                                    cs.cursor_y = 0;
                                    cs.cursor_x = 0;
                                }
                                KeyCode::Char('G') => {
                                    cs.scroll_offset = 0;
                                    cs.cursor_y = pane_h.saturating_sub(1);
                                }
                                // Shift-V: Visual Line mode (highlights whole row!)
                                KeyCode::Char('V') => {
                                    let abs_y = vt_screens.get(&active_pane).map(|vt| vt.grid().abs_line(cs.cursor_y, cs.scroll_offset)).unwrap_or(0);
                                    if matches!(cs.mode, SelectionKind::Line) {
                                        cs.mode = SelectionKind::None;
                                    } else {
                                        cs.mode = SelectionKind::Line;
                                        cs.anchor_abs_y = abs_y;
                                        cs.anchor_x = 0;
                                    }
                                }
                                KeyCode::Char('v') => {
                                    let abs_y = vt_screens.get(&active_pane).map(|vt| vt.grid().abs_line(cs.cursor_y, cs.scroll_offset)).unwrap_or(0);
                                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                                        if matches!(cs.mode, SelectionKind::Line) {
                                            cs.mode = SelectionKind::None;
                                        } else {
                                            cs.mode = SelectionKind::Line;
                                            cs.anchor_abs_y = abs_y;
                                            cs.anchor_x = 0;
                                        }
                                    } else if matches!(cs.mode, SelectionKind::Character) {
                                        cs.mode = SelectionKind::None;
                                    } else {
                                        cs.mode = SelectionKind::Character;
                                        cs.anchor_abs_y = abs_y;
                                        cs.anchor_x = cs.cursor_x;
                                    }
                                }
                                // y or Enter: Yank selection to clipboard and exit copy mode
                                KeyCode::Char('y') | KeyCode::Enter => {
                                    if let Some(vt) = vt_screens.get(&active_pane) {
                                        let text = cs.yank_text(vt.grid());
                                        super::render::copy_to_clipboard(&text);
                                    }
                                    copy_state = None;
                                }
                                KeyCode::Esc => {
                                    if !matches!(cs.mode, SelectionKind::None) {
                                        cs.mode = SelectionKind::None;
                                    } else {
                                        copy_state = None;
                                    }
                                }
                                KeyCode::Char('q') => {
                                    copy_state = None;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        if leader_active {
                            leader_active = false;
                            layout_cache.dirty = true;

                            let is_split_vert = key.code == KeyCode::Char('%')
                                || key.code == KeyCode::Char('|')
                                || (key.code == KeyCode::Char('5') && key.modifiers.contains(KeyModifiers::SHIFT));

                            let is_split_horz = key.code == KeyCode::Char('"')
                                || key.code == KeyCode::Char('-')
                                || (key.code == KeyCode::Char('\'') && key.modifiers.contains(KeyModifiers::SHIFT));

                            let cur_cwd = if !active_pane.is_empty() {
                                client.get_cwd(&active_pane).await.ok().flatten()
                            } else {
                                None
                            }.or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));

                            if is_split_vert {
                                zoomed_pane = None;
                                split_counter += 1;
                                let new_name = format!("pane-{}", split_counter);
                                let shell = default_shell();
                                let target = if active_pane.is_empty() { None } else { Some(active_pane.clone()) };
                                let _ = client
                                    .split(SplitOpts {
                                        target,
                                        name: new_name,
                                        command: shell,
                                        args: vec![],
                                        pty: true,
                                        direction: Direction::Horizontal,
                                        cwd: cur_cwd,
                                        focus: true,
                                    })
                                    .await;
                            } else if is_split_horz {
                                zoomed_pane = None;
                                split_counter += 1;
                                let new_name = format!("pane-{}", split_counter);
                                let shell = default_shell();
                                let target = if active_pane.is_empty() { None } else { Some(active_pane.clone()) };
                                let _ = client
                                    .split(SplitOpts {
                                        target,
                                        name: new_name,
                                        command: shell,
                                        args: vec![],
                                        pty: true,
                                        direction: Direction::Vertical,
                                        cwd: cur_cwd,
                                        focus: true,
                                    })
                                    .await;
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
                                    // Directional pane navigation (hjkl or arrow keys)
                                    KeyCode::Char('h') | KeyCode::Left => {
                                        if let Some(target) = find_directional_pane(&panes, &active_pane, MoveDir::Left) {
                                            let _ = client.focus(&target).await;
                                        }
                                    }
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        if let Some(target) = find_directional_pane(&panes, &active_pane, MoveDir::Down) {
                                            let _ = client.focus(&target).await;
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Up => {
                                        if let Some(target) = find_directional_pane(&panes, &active_pane, MoveDir::Up) {
                                            let _ = client.focus(&target).await;
                                        }
                                    }
                                    KeyCode::Char('l') | KeyCode::Right => {
                                        if let Some(target) = find_directional_pane(&panes, &active_pane, MoveDir::Right) {
                                            let _ = client.focus(&target).await;
                                        }
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
                                        let cur_cwd = if !active_pane.is_empty() {
                                            client.get_cwd(&active_pane).await.ok().flatten()
                                        } else {
                                            None
                                        }.or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));
                                        let _ = client
                                            .new_window(NewWindowOpts {
                                                name: None,
                                                command: shell,
                                                args: vec![],
                                                pty: true,
                                                cwd: cur_cwd,
                                            })
                                            .await;
                                    }
                                    // Kill pane (tmux x)
                                    KeyCode::Char('x') => {
                                        if !active_pane.is_empty() {
                                            zoomed_pane = None;
                                            let _ = client.kill(&active_pane).await;
                                        }
                                    }
                                    // Detach or Quit (tmux d or q)
                                    KeyCode::Char('d') | KeyCode::Char('q') => {
                                        return Ok(());
                                    }
                                    // Enter copy mode (tmux [)
                                    KeyCode::Char('[') => {
                                        let (cx, cy) = if let Some(vt) = vt_screens.get(&active_pane) {
                                            (vt.grid().cursor.x, vt.grid().cursor.y)
                                        } else {
                                            (0, 0)
                                        };
                                        let abs_y = if let Some(vt) = vt_screens.get(&active_pane) {
                                            vt.grid().abs_line(cy, 0)
                                        } else {
                                            0
                                        };
                                        copy_state = Some(CopyState {
                                            cursor_x: cx,
                                            cursor_y: cy,
                                            scroll_offset: 0,
                                            anchor_x: cx,
                                            anchor_abs_y: abs_y,
                                            mode: SelectionKind::None,
                                        });
                                    }
                                    KeyCode::Esc => {
                                        // Cancel shortcut popup
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            // Forward key input directly to active pane with instant batching
                            if !active_pane.is_empty() {
                                let mut payload = key_to_bytes(key.code, key.modifiers);
                                while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
                                    if let Ok(Event::Key(next_key)) = crossterm::event::read() {
                                        payload.extend(key_to_bytes(next_key.code, next_key.modifiers));
                                    } else {
                                        break;
                                    }
                                }
                                if !payload.is_empty() {
                                    let text = String::from_utf8_lossy(&payload).into_owned();
                                    let _ = client.send_input(&active_pane, &text).await;
                                }
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        let mut latest_mouse = mouse;
                        while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
                            if let Ok(Event::Mouse(next_mouse)) = crossterm::event::read() {
                                if matches!(next_mouse.kind, MouseEventKind::Drag(_) | MouseEventKind::Moved) {
                                    latest_mouse = next_mouse;
                                } else {
                                    let dirty = handle_mouse(
                                        latest_mouse.kind,
                                        latest_mouse.column,
                                        latest_mouse.row,
                                        &mut client,
                                        MouseContext {
                                            panes: &panes,
                                            active_pane: &active_pane,
                                            active_rect,
                                            copy_state: &mut copy_state,
                                            vt_screens: &vt_screens,
                                        },
                                    ).await;
                                    if dirty {
                                        layout_cache.dirty = true;
                                    }
                                    latest_mouse = next_mouse;
                                }
                            } else {
                                break;
                            }
                        }

                        let dirty = handle_mouse(
                            latest_mouse.kind,
                            latest_mouse.column,
                            latest_mouse.row,
                            &mut client,
                            MouseContext {
                                panes: &panes,
                                active_pane: &active_pane,
                                active_rect,
                                copy_state: &mut copy_state,
                                vt_screens: &vt_screens,
                            },
                        ).await;
                        if dirty {
                            layout_cache.dirty = true;
                        }
                    }
                    Event::Paste(pasted) => {
                        if !active_pane.is_empty() {
                            let _ = client.send_input(&active_pane, &pasted).await;
                        }
                    }
                    Event::Resize(..) => {
                        layout_cache.dirty = true;
                    }
                    _ => {}
                }
            }

            Some(stream_event) = rx_events.recv() => {
                let mut events = vec![stream_event];
                while let Ok(ev) = rx_events.try_recv() {
                    events.push(ev);
                }

                for ev in events {
                    if apply_stream_event(ev, &mut vt_screens) {
                        layout_cache.dirty = true;
                    }
                }
                if vt_screens.is_empty() {
                    return Ok(());
                }
                wait_for_frame(last_stream_render, frame_budget).await;
                last_stream_render = Some(std::time::Instant::now());
            }
            _ = tick_timer.tick() => {}
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

async fn wait_for_frame(last_render: Option<std::time::Instant>, budget: Duration) {
    if let Some(lr) = last_render {
        if lr.elapsed() < budget {
            tokio::time::sleep(budget.saturating_sub(lr.elapsed())).await;
        }
    }
}

async fn sync_pane_dimensions(
    client: &mut Client,
    panes: &[(String, Rect, bool)],
    pane_dimensions: &mut HashMap<String, (u16, u16)>,
    vt_screens: &mut HashMap<String, VtScreen>,
) {
    for (name, rect, _) in panes {
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        let current_dim = (rect.width, rect.height);
        let prev_dim = pane_dimensions.get(name).copied();
        if prev_dim != Some(current_dim) {
            pane_dimensions.insert(name.clone(), current_dim);
            let _ = client.resize_pane(name, rect.width, rect.height).await;
            if let Ok(logs) = client.logs(name, None).await {
                let mut new_vt = VtScreen::new(rect.width as usize, rect.height as usize);
                new_vt.feed(logs.as_bytes());
                vt_screens.insert(name.clone(), new_vt);
            }
        }
    }
}

fn apply_stream_event(ev: StreamEvent, vt_screens: &mut HashMap<String, VtScreen>) -> bool {
    match ev {
        StreamEvent::Output { name, data } => {
            let vt = vt_screens
                .entry(name)
                .or_insert_with(|| VtScreen::new(80, 24));
            vt.feed(data.as_bytes());
            false
        }
        StreamEvent::Exit { name, .. } => {
            vt_screens.remove(&name);
            true
        }
        _ => false,
    }
}

struct LayoutCache {
    dirty: bool,
    panes: Vec<(String, Rect, bool)>,
    dividers: Vec<Divider>,
    tabs: Vec<WindowTab>,
}

impl LayoutCache {
    fn new() -> Self {
        Self {
            dirty: true,
            panes: Vec::new(),
            dividers: Vec::new(),
            tabs: Vec::new(),
        }
    }

    async fn sync(&mut self, client: &mut Client, term_w: u16, layout_h: u16) -> bool {
        if self.dirty || self.panes.is_empty() {
            if let Ok((p, d, t)) = client.layout(term_w, layout_h).await {
                self.panes = p;
                self.dividers = d;
                self.tabs = t;
                self.dirty = false;
            }
        }
        !self.panes.is_empty()
    }
}

struct MouseContext<'a> {
    panes: &'a [(String, Rect, bool)],
    active_pane: &'a str,
    active_rect: Option<Rect>,
    copy_state: &'a mut Option<CopyState>,
    vt_screens: &'a HashMap<String, VtScreen>,
}

async fn handle_mouse(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    client: &mut Client,
    ctx: MouseContext<'_>,
) -> bool {
    let mut layout_dirty = false;
    match kind {
        MouseEventKind::Moved => {}
        MouseEventKind::Down(MouseButton::Left) => {
            for (name, rect, _) in ctx.panes {
                if rect.contains(col, row) {
                    if *name != ctx.active_pane {
                        let _ = client.focus(name).await;
                        layout_dirty = true;
                    }
                    let local_x = (col.saturating_sub(rect.x) as usize).min(rect.width.saturating_sub(1) as usize);
                    let local_y = (row.saturating_sub(rect.y) as usize).min(rect.height.saturating_sub(1) as usize);
                    let abs_y = if let Some(vt) = ctx.vt_screens.get(name) {
                        vt.grid().abs_line(local_y, 0)
                    } else {
                        local_y
                    };
                    *ctx.copy_state = Some(CopyState {
                        cursor_x: local_x,
                        cursor_y: local_y,
                        scroll_offset: 0,
                        anchor_x: local_x,
                        anchor_abs_y: abs_y,
                        mode: SelectionKind::Character,
                    });
                    break;
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let (Some(rect), Some(cs)) = (ctx.active_rect, ctx.copy_state) {
                let local_x = (col.saturating_sub(rect.x) as usize).min(rect.width.saturating_sub(1) as usize);
                let local_y = (row.saturating_sub(rect.y) as usize).min(rect.height.saturating_sub(1) as usize);
                cs.cursor_x = local_x;
                cs.cursor_y = local_y;
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let (Some(cs), Some(vt)) = (&*ctx.copy_state, ctx.vt_screens.get(ctx.active_pane)) {
                let cursor_abs_y = vt.grid().abs_line(cs.cursor_y, cs.scroll_offset);
                if cs.anchor_x != cs.cursor_x || cs.anchor_abs_y != cursor_abs_y {
                    let text = cs.yank_text(vt.grid());
                    if !text.trim().is_empty() {
                        super::render::copy_to_clipboard(&text);
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            let hist_len = ctx.vt_screens.get(ctx.active_pane).map(|vt| vt.grid().history_len()).unwrap_or(0);
            if ctx.copy_state.is_none() {
                let (cx, cy) = ctx.vt_screens.get(ctx.active_pane).map(|vt| (vt.grid().cursor.x, vt.grid().cursor.y)).unwrap_or((0, 0));
                let abs_y = ctx.vt_screens.get(ctx.active_pane).map(|vt| vt.grid().abs_line(cy, 0)).unwrap_or(0);
                let initial_scroll = 3.min(hist_len);
                let initial_y = if hist_len == 0 { cy.saturating_sub(3) } else { cy };
                *ctx.copy_state = Some(CopyState {
                    cursor_x: cx,
                    cursor_y: initial_y,
                    scroll_offset: initial_scroll,
                    anchor_x: cx,
                    anchor_abs_y: abs_y,
                    mode: SelectionKind::None,
                });
            } else if let Some(cs) = ctx.copy_state {
                if hist_len > 0 && cs.scroll_offset < hist_len {
                    cs.scroll_offset = (cs.scroll_offset + 3).min(hist_len);
                } else {
                    cs.cursor_y = cs.cursor_y.saturating_sub(3);
                }
            }
        }
        MouseEventKind::ScrollDown => {
            if let Some(cs) = ctx.copy_state {
                if cs.scroll_offset > 0 {
                    cs.scroll_offset = cs.scroll_offset.saturating_sub(3);
                } else {
                    let pane_h = ctx.active_rect.map(|r| r.height as usize).unwrap_or(24);
                    cs.cursor_y = (cs.cursor_y + 3).min(pane_h.saturating_sub(1));
                }
            }
        }
        _ => {}
    }
    layout_dirty
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveDir {
    Left,
    Down,
    Up,
    Right,
}

fn find_directional_pane(
    panes: &[(String, Rect, bool)],
    active_pane: &str,
    dir: MoveDir,
) -> Option<String> {
    let current_rect = panes
        .iter()
        .find(|(name, ..)| name == active_pane)
        .map(|(.., rect, _)| *rect)?;

    let cx = current_rect.x as i32 + current_rect.width as i32 / 2;
    let cy = current_rect.y as i32 + current_rect.height as i32 / 2;

    let mut best_candidate: Option<(&String, i32)> = None;

    for (name, rect, _) in panes {
        if name == active_pane {
            continue;
        }

        let ocx = rect.x as i32 + rect.width as i32 / 2;
        let ocy = rect.y as i32 + rect.height as i32 / 2;

        let valid = match dir {
            MoveDir::Left => (rect.x + rect.width) <= (current_rect.x + 2),
            MoveDir::Right => (rect.x + 2) >= (current_rect.x + current_rect.width),
            MoveDir::Up => (rect.y + rect.height) <= (current_rect.y + 2),
            MoveDir::Down => (rect.y + 2) >= (current_rect.y + current_rect.height),
        };

        if valid {
            let dx = (cx - ocx).abs();
            let dy = (cy - ocy).abs();
            let score = match dir {
                MoveDir::Left | MoveDir::Right => dx + dy * 3,
                MoveDir::Up | MoveDir::Down => dy + dx * 3,
            };

            if best_candidate.as_ref().map_or(true, |(_, best_score)| score < *best_score) {
                best_candidate = Some((name, score));
            }
        }
    }

    best_candidate.map(|(name, _)| name.clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionKind {
    None,
    Character,
    Line,
}

#[derive(Debug, Clone)]
struct CopyState {
    cursor_x: usize,
    cursor_y: usize,
    scroll_offset: usize,
    anchor_x: usize,
    anchor_abs_y: usize,
    mode: SelectionKind,
}

impl CopyState {
    fn selection_range(&self, grid: &crate::vt::grid::Grid) -> Option<SelectionRange> {
        match self.mode {
            SelectionKind::None => None,
            SelectionKind::Line => {
                let cursor_abs_y = grid.abs_line(self.cursor_y, self.scroll_offset);
                Some(SelectionRange::Line {
                    start_abs_y: self.anchor_abs_y,
                    end_abs_y: cursor_abs_y,
                })
            }
            SelectionKind::Character => {
                let cursor_abs_y = grid.abs_line(self.cursor_y, self.scroll_offset);
                Some(SelectionRange::Character {
                    anchor_x: self.anchor_x,
                    anchor_abs_y: self.anchor_abs_y,
                    cursor_x: self.cursor_x,
                    cursor_abs_y,
                })
            }
        }
    }

    fn yank_text(&self, grid: &crate::vt::grid::Grid) -> String {
        let cursor_abs_y = grid.abs_line(self.cursor_y, self.scroll_offset);
        match self.mode {
            SelectionKind::Line => {
                let min_y = self.anchor_abs_y.min(cursor_abs_y);
                let max_y = self.anchor_abs_y.max(cursor_abs_y);
                let mut lines = Vec::new();
                for y in min_y..=max_y {
                    if let Some(cells) = grid.get_line_cells(y) {
                        let row_str: String = cells.into_iter().map(|c| c.c).collect();
                        lines.push(row_str.trim_end().to_string());
                    }
                }
                lines.join("\n")
            }
            SelectionKind::Character => {
                let ((sx, sy), (ex, ey)) = if (self.anchor_abs_y, self.anchor_x) <= (cursor_abs_y, self.cursor_x) {
                    ((self.anchor_x, self.anchor_abs_y), (self.cursor_x, cursor_abs_y))
                } else {
                    ((self.cursor_x, cursor_abs_y), (self.anchor_x, self.anchor_abs_y))
                };

                let mut out = String::new();
                for y in sy..=ey {
                    if let Some(cells) = grid.get_line_cells(y) {
                        let from_x = if y == sy { sx } else { 0 };
                        let to_x = if y == ey { ex } else { cells.len().saturating_sub(1) };
                        if from_x <= to_x && from_x < cells.len() {
                            let end_bound = (to_x + 1).min(cells.len());
                            let row_str: String = cells[from_x..end_bound]
                                .iter()
                                .map(|c| c.c)
                                .collect();
                            if y < ey {
                                out.push_str(row_str.trim_end());
                                out.push('\n');
                            } else {
                                out.push_str(&row_str);
                            }
                        }
                    }
                }
                out
            }
            SelectionKind::None => {
                if let Some(cells) = grid.get_line_cells(cursor_abs_y) {
                    let row_str: String = cells.into_iter().map(|c| c.c).collect();
                    row_str.trim_end().to_string()
                } else {
                    String::new()
                }
            }
        }
    }
}

fn is_dimmed(panes_count: usize, is_zoomed: bool, is_focused: bool) -> bool {
    panes_count > 1 && !is_zoomed && !is_focused
}
