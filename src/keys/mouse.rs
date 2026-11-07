use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

use super::csi::weighted_mod_bits;
use super::{MouseTracking, TermModes};

/// Encodes a mouse event the way the pane asked for: SGR when enabled, otherwise X10. `None` when
/// the tracking mode does not report this event.
pub fn encode_mouse_event(
    modes: &TermModes,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    let reported = match (modes.mouse_tracking, kind) {
        (MouseTracking::None, _) => false,
        (MouseTracking::X10, MouseEventKind::Down(_)) => true,
        (MouseTracking::X10, _) => false,
        (MouseTracking::ButtonEvent, MouseEventKind::Moved) => false,
        (MouseTracking::ButtonEvent | MouseTracking::AnyEvent, _) => true,
    };
    if !reported {
        None
    } else if modes.mouse_sgr {
        encode_sgr_mouse(kind, col, row, mods)
    } else {
        encode_x10_mouse(kind, col, row, mods)
    }
}

pub fn encode_sgr_mouse(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    let (button, release) = match kind {
        MouseEventKind::Down(b) => (button_code(b), false),
        MouseEventKind::Up(b) => (button_code(b), true),
        MouseEventKind::Drag(b) => (32 + button_code(b), false),
        MouseEventKind::Moved => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
    };
    let btn = with_mouse_mods(button, mods);
    let final_char = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{btn};{col};{row}{final_char}").into_bytes())
}

pub fn encode_x10_mouse(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> Option<Vec<u8>> {
    let button = match kind {
        MouseEventKind::Down(b) => button_code(b),
        MouseEventKind::Up(_) => 3,
        MouseEventKind::Drag(b) => 32 + button_code(b),
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        _ => return None,
    };
    let btn = with_mouse_mods(button, mods);
    let col = col.clamp(1, 223) as u8;
    let row = row.clamp(1, 223) as u8;
    Some(vec![0x1b, b'[', b'M', 32 + btn, 32 + col, 32 + row])
}

fn button_code(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Adds the shift (4), alt (8) and ctrl (16) bits to a button code.
fn with_mouse_mods(button: u8, mods: KeyModifiers) -> u8 {
    button + weighted_mod_bits(mods, 4, 8, 16)
}
