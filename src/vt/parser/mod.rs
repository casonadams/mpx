mod csi;
mod dcs;
mod modes;
mod osc;
mod screen;
mod sgr;

use vte::{Params, Perform};

use super::grid::Grid;
use crate::keys::TermModes;

pub use screen::{Alert, VtScreen};

pub struct VtHandler {
    pub grid: Grid,
    pub responses: Vec<Vec<u8>>,
    pub passthroughs: Vec<Vec<u8>>,
    pub bg_color: Option<String>,
    modes: TermModes,
    /// Kitty keyboard flags the program pushed (`CSI > n u`), restored by `CSI < n u`.
    kitty_stack: Vec<u8>,
    /// A synchronized update (DEC 2026) the program has begun and not ended.
    sync_output: bool,
    title: Option<String>,
    alerts: Vec<Alert>,
    last_char: Option<char>,
    dcs_buf: Option<Vec<u8>>,
}

impl VtHandler {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            responses: Vec::new(),
            passthroughs: Vec::new(),
            bg_color: None,
            modes: TermModes::default(),
            kitty_stack: Vec::new(),
            sync_output: false,
            title: None,
            alerts: Vec::new(),
            last_char: None,
            dcs_buf: None,
        }
    }

    fn reverse_index(&mut self) {
        if self.grid.cursor.y <= self.grid.scroll_top {
            self.grid.scroll_down(1);
        } else {
            self.grid.cursor.y -= 1;
        }
    }

    fn full_reset(&mut self) {
        let history = std::mem::take(&mut self.grid.history);
        let limit = self.grid.history_limit;
        self.grid = Grid::new(self.grid.cols, self.grid.rows);
        self.grid.history = history;
        self.grid.history_limit = limit;
        self.modes = TermModes::default();
        self.kitty_stack.clear();
        self.sync_output = false;
        self.title = None;
        self.bg_color = None;
        self.dcs_buf = None;
        self.last_char = None;
    }
}

/// CSI forms with a private intermediate (`CSI > 4;2 m`, `CSI ? 6 n`) share finals with SGR,
/// cursor restore and DSR but mean something else, so only `c`, `h` and `l` may carry one.
fn is_private_form(intermediates: &[u8], action: char) -> bool {
    !intermediates.is_empty() && !matches!(action, 'c' | 'h' | 'l')
}

impl Perform for VtHandler {
    fn print(&mut self, c: char) {
        self.last_char = Some(c);
        self.grid.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.grid.newline(),
            b'\r' => self.grid.carriage_return(),
            0x07 => self.alerts.push(Alert::Bell),
            0x08 => self.grid.backspace(),
            b'\t' => {
                let next_tab = (self.grid.cursor.x / 8 + 1) * 8;
                self.grid.cursor.x = next_tab.min(self.grid.cols.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if self.keyboard_csi(params, intermediates, action)
            || is_private_form(intermediates, action)
        {
            return;
        }
        match action {
            'm' => self.handle_sgr(params),
            'H' | 'f' | 'A' | 'B' | 'C' | 'D' => self.csi_cursor_move(action, params),
            'J' | 'K' => self.csi_erase(action, params),
            'S' | 'T' | 'M' | 'L' => self.csi_scroll(action, params),
            's' => self.csi_save_cursor(params),
            'u' => self.csi_restore_cursor(),
            'n' => self.csi_device_status(params),
            'c' => self.csi_device_attributes(intermediates),
            'h' | 'l' => self.csi_set_mode(action, intermediates, params),
            'r' => self.csi_set_margins(params),
            _ => self.handle_csi_cursor_edit(action, params),
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        self.dcs_hook(action);
    }

    fn put(&mut self, byte: u8) {
        self.dcs_put(byte);
    }

    fn unhook(&mut self) {
        self.dcs_unhook();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.handle_osc(params, bell_terminated);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            ([], b'7') => self.grid.save_cursor(),
            ([], b'8') => self.grid.restore_cursor(),
            ([], b'c') => self.full_reset(),
            ([], b'M') => self.reverse_index(),
            _ => {}
        }
    }
}
