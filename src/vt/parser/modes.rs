use vte::Params;

use super::VtHandler;
use crate::keys::MouseTracking;

/// Kitty keyboard pushes kept; the oldest is dropped past this, as terminals bound their stacks.
const KITTY_STACK_DEPTH: usize = 16;

/// The kitty keyboard flags a program sets: the five defined bits, nothing else.
fn kitty_flags(value: u16) -> u8 {
    (value & 0x1f) as u8
}

impl VtHandler {
    pub(super) fn csi_set_mode(&mut self, action: char, intermediates: &[u8], params: &Params) {
        if intermediates == b"?" {
            self.handle_mode(params, action == 'h');
        }
    }

    fn handle_mode(&mut self, params: &Params, set: bool) {
        for param in params.iter() {
            match param[0] {
                1 => self.modes.app_cursor = set,
                25 => self.grid.cursor.visible = set,
                1049 | 1047 | 47 => {
                    self.grid.set_alternate_screen(set);
                    self.grid.clear_screen(2);
                }
                69 => self.grid.set_lr_margins_mode(set),
                1000 => self.set_mouse_tracking(set, MouseTracking::X10),
                1002 => self.set_mouse_tracking(set, MouseTracking::ButtonEvent),
                1003 => self.set_mouse_tracking(set, MouseTracking::AnyEvent),
                1006 => self.modes.mouse_sgr = set,
                1004 => self.modes.focus_events = set,
                2004 => self.modes.bracketed_paste = set,
                2026 => self.sync_output = set,
                _ => {}
            }
        }
    }

    fn set_mouse_tracking(&mut self, set: bool, mode: MouseTracking) {
        self.modes.mouse_tracking = if set { mode } else { MouseTracking::None };
    }

    /// CSI forms that set input modes: xterm modifyOtherKeys (`CSI > 4 ; n m`) and the kitty
    /// keyboard protocol (`CSI > n u`, `CSI < n u`, `CSI = n ; m u`, `CSI ? u`). True when the
    /// sequence was one of them.
    pub(super) fn keyboard_csi(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        action: char,
    ) -> bool {
        let arg = |i: usize, default: u16| params.iter().nth(i).map_or(default, |p| p[0]);
        match (intermediates, action) {
            ([b'>'], 'm') => {
                if arg(0, 0) == 4 {
                    self.modes.modify_other_keys = arg(1, 0).min(2) as u8;
                }
                true
            }
            ([b'>'], 'u') => {
                if self.kitty_stack.len() == KITTY_STACK_DEPTH {
                    self.kitty_stack.remove(0);
                }
                self.kitty_stack.push(self.modes.kitty_keys);
                self.modes.kitty_keys = kitty_flags(arg(0, 0));
                true
            }
            ([b'<'], 'u') => {
                for _ in 0..arg(0, 1).max(1) {
                    self.modes.kitty_keys = self.kitty_stack.pop().unwrap_or(0);
                }
                true
            }
            ([b'='], 'u') => {
                let flags = kitty_flags(arg(0, 0));
                let current = self.modes.kitty_keys;
                self.modes.kitty_keys = match arg(1, 1) {
                    2 => current | flags,
                    3 => current & !flags,
                    _ => flags,
                };
                true
            }
            ([b'?'], 'u') => {
                self.responses
                    .push(format!("\x1b[?{}u", self.modes.kitty_keys).into_bytes());
                true
            }
            _ => false,
        }
    }
}
