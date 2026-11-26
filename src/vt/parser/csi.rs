use vte::Params;

use super::VtHandler;
use crate::vt::grid::CursorShape;

/// The first parameter at position `i`, when it is present.
fn param(params: &Params, i: usize) -> Option<usize> {
    params.iter().nth(i).map(|p| p[0] as usize)
}

/// The first parameter as a count, `default` when it is absent.
fn first_param(params: &Params, default: usize) -> usize {
    param(params, 0).unwrap_or(default)
}

fn count_param(params: &Params) -> usize {
    first_param(params, 1).max(1)
}

impl VtHandler {
    fn cursor_up(&mut self, n: usize) {
        let grid = &mut self.grid;
        let floor = if grid.cursor.y >= grid.scroll_top {
            grid.scroll_top
        } else {
            0
        };
        grid.cursor.y = grid.cursor.y.saturating_sub(n).max(floor);
    }

    fn cursor_down(&mut self, n: usize) {
        let grid = &mut self.grid;
        let ceil = if grid.cursor.y <= grid.scroll_bottom {
            grid.scroll_bottom
        } else {
            grid.rows - 1
        };
        grid.cursor.y = (grid.cursor.y + n).min(ceil);
    }

    pub(super) fn handle_csi_cursor_edit(&mut self, action: char, params: &Params) {
        let n = count_param(params);
        match action {
            'G' | '`' => {
                let y = self.grid.cursor.y;
                self.grid.move_cursor(n - 1, y);
            }
            'd' => {
                let x = self.grid.cursor.x;
                let (_, y) = self.grid.addressed_position(0, n - 1);
                self.grid.move_cursor(x, y);
            }
            'a' => {
                let x = self.grid.cursor.x + n;
                let y = self.grid.cursor.y;
                self.grid.move_cursor(x, y);
            }
            'E' => {
                self.cursor_down(n);
                self.grid.cursor.x = 0;
            }
            'F' => {
                self.cursor_up(n);
                self.grid.cursor.x = 0;
            }
            'Z' => self.grid.tab_backward(n),
            'I' => self.grid.tab_forward(n),
            'g' => self.grid.clear_tab_stops(first_param(params, 0)),
            'X' => self.grid.erase_chars(n),
            '@' => self.grid.insert_blanks(n),
            'P' => self.grid.delete_chars(n),
            'b' => {
                if let Some(c) = self.last_char {
                    for _ in 0..n {
                        self.grid.put_char(c);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn csi_cursor_move(&mut self, action: char, params: &Params) {
        match action {
            'H' | 'f' => {
                let row = first_param(params, 1);
                let col = param(params, 1).unwrap_or(1);
                let (x, y) = self
                    .grid
                    .addressed_position(col.saturating_sub(1), row.saturating_sub(1));
                self.grid.move_cursor(x, y);
            }
            'A' => self.cursor_up(count_param(params)),
            'B' => self.cursor_down(count_param(params)),
            'C' => {
                let count = count_param(params);
                let cols = self.grid.cols.saturating_sub(1);
                self.grid.cursor.x = (self.grid.cursor.x + count).min(cols);
            }
            _ => {
                let count = count_param(params);
                let x = self.grid.cursor.x.min(self.grid.cols - 1);
                self.grid.cursor.x = x.saturating_sub(count);
            }
        }
    }

    pub(super) fn csi_erase(&mut self, action: char, params: &Params) {
        let mode = first_param(params, 0);
        if action == 'J' {
            self.grid.clear_screen(mode);
        } else {
            self.grid.clear_line(mode);
        }
    }

    pub(super) fn csi_scroll(&mut self, action: char, params: &Params) {
        let count = count_param(params);
        match action {
            'S' => self.grid.scroll_up(count),
            'T' => self.grid.scroll_down(count),
            'M' => self.grid.delete_lines(count),
            _ => self.grid.insert_lines(count),
        }
    }

    /// `CSI s`: DECSLRM left/right margins while DECLRMM is set, otherwise save the cursor.
    pub(super) fn csi_save_cursor(&mut self, params: &Params) {
        if self.grid.lr_margins {
            let value = |i: usize| param(params, i).filter(|&v| v > 0);
            self.grid.set_lr_margins(value(0), value(1));
        } else {
            self.save_cursor();
        }
    }

    pub(super) fn csi_restore_cursor(&mut self) {
        self.restore_cursor();
    }

    /// `CSI 5 n` reports terminal status; `CSI 6 n` reports the cursor position (DSR).
    pub(super) fn csi_device_status(&mut self, params: &Params) {
        match first_param(params, 0) {
            // Cursor Position Report (DSR): ESC [ row ; col R
            6 => {
                let origin = self.grid.addressed_position(0, 0);
                let r = (self.grid.cursor.y + 1).saturating_sub(origin.1);
                let c = (self.grid.cursor.x + 1).saturating_sub(origin.0);
                self.responses
                    .push(format!("\x1b[{};{}R", r, c).into_bytes());
            }
            // Terminal status OK
            5 => self.responses.push(b"\x1b[0n".to_vec()),
            _ => {}
        }
    }

    pub(super) fn csi_device_attributes(&mut self, intermediates: &[u8]) {
        // Secondary Device Attributes for `CSI > c`, otherwise Primary (VT220 compatible).
        let reply: &[u8] = if intermediates == b">" {
            b"\x1b[>0;10;0c"
        } else {
            b"\x1b[?62;c"
        };
        self.responses.push(reply.to_vec());
    }

    /// `CSI Ps SP q` (DECSCUSR) picks the cursor shape; an unknown `Ps` leaves it as it is.
    pub(super) fn csi_cursor_shape(&mut self, params: &Params) {
        let ps = params.iter().next().map_or(0, |p| p[0]);
        if let Some(shape) = CursorShape::from_param(ps) {
            self.grid.cursor_shape = shape;
        }
    }

    pub(super) fn csi_set_margins(&mut self, params: &Params) {
        let grid = &mut self.grid;
        let top = first_param(params, 1).max(1);
        let bottom = param(params, 1).filter(|&b| b != 0).unwrap_or(grid.rows);
        let top = top.saturating_sub(1);
        let bottom = bottom.saturating_sub(1).min(grid.rows.saturating_sub(1));
        if top < bottom {
            grid.scroll_top = top;
            grid.scroll_bottom = bottom;
        }
        grid.home_cursor();
    }
}
