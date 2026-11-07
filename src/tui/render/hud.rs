use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};

use super::Compositor;
use crate::layout::geometry::Rect;

/// Where the prefix HUD sits: bottom-right above the status row; `None` when the screen is too small.
pub fn hud_rect(screen_w: u16, screen_h: u16) -> Option<Rect> {
    let box_w: u16 = 30;
    let box_h: u16 = (HUD_ITEMS.len() as u16 + 2).min(screen_h.saturating_sub(2));
    if box_w >= screen_w || box_h < 4 {
        return None;
    }
    let box_x = screen_w.saturating_sub(box_w + 2);
    let box_y = screen_h.saturating_sub(box_h + 1);
    Some(Rect::new(box_x, box_y, box_w, box_h))
}

const HUD_ITEMS: &[(&str, &str)] = &[
    ("c", "new window"),
    ("p", "previous window"),
    ("n", "next window"),
    ("%", "split vertical"),
    ("\"", "split horizontal"),
    ("h", "focus left"),
    ("j", "focus down"),
    ("k", "focus up"),
    ("l", "focus right"),
    ("o", "next pane"),
    ("m", "mark pane"),
    ("z", "toggle zoom"),
    ("[", "copy mode"),
    ("]", "paste buffer"),
    ("f", "find window/pane"),
    ("x", "kill pane"),
    ("d", "detach"),
    ("s", "sessions"),
    ("$", "rename session"),
    (",", "rename window"),
    ("w", "windows"),
    ("?", "help"),
];

fn draw_hud_header<W: Write>(w: &mut W, x: u16, y: u16, box_w: u16) -> io::Result<()> {
    queue!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("╭"),
        Print("─".repeat(box_w.saturating_sub(2) as usize)),
        Print("╮"),
        ResetColor
    )
}

fn draw_hud_row<W: Write>(
    w: &mut W,
    (x, y): (u16, u16),
    box_w: u16,
    (key, desc): (&str, &str),
) -> io::Result<()> {
    queue!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("│ "),
        SetForegroundColor(Color::Yellow),
        Print(format!("{:<2}", key)),
        SetForegroundColor(Color::DarkGrey),
        Print(" → "),
        SetForegroundColor(Color::Cyan),
        Print(format!("{:<18}", desc)),
    )?;
    let used = 2 + 2 + 3 + 18;
    let pad = (box_w as usize).saturating_sub(used + 1);
    queue!(
        w,
        Print(" ".repeat(pad)),
        SetForegroundColor(Color::DarkCyan),
        Print("│"),
        ResetColor
    )
}

fn draw_hud_footer<W: Write>(w: &mut W, x: u16, y: u16, box_w: u16) -> io::Result<()> {
    let footer_text = " ESC close ";
    let dash_count = (box_w as usize).saturating_sub(footer_text.len() + 2);
    queue!(
        w,
        MoveTo(x, y),
        SetBackgroundColor(Color::Black),
        SetForegroundColor(Color::DarkCyan),
        Print("╰"),
        Print("─".repeat(dash_count)),
        SetForegroundColor(Color::Yellow),
        Print(" ESC "),
        SetForegroundColor(Color::DarkGrey),
        Print("close"),
        SetForegroundColor(Color::DarkCyan),
        Print(" ╯"),
        ResetColor
    )
}

impl Compositor {
    /// Floating popup listing the prefix shortcuts, anchored bottom-right above the status row.
    pub fn draw_shortcut_hud<W: Write>(w: &mut W, screen_w: u16, screen_h: u16) -> io::Result<()> {
        let Some(area) = hud_rect(screen_w, screen_h) else {
            return Ok(());
        };
        let (box_x, box_y, box_w, box_h) = (area.x, area.y, area.width, area.height);
        draw_hud_header(w, box_x, box_y, box_w)?;
        for (i, (key, desc)) in HUD_ITEMS.iter().take((box_h - 2) as usize).enumerate() {
            draw_hud_row(w, (box_x, box_y + 1 + i as u16), box_w, (key, desc))?;
        }
        draw_hud_footer(w, box_x, box_y + box_h - 1, box_w)
    }
}
