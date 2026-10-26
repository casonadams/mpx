use vte::{Params, Perform};

use super::grid::{CellStyle, Color, Grid};

pub struct VtHandler {
    pub grid: Grid,
    pub responses: Vec<Vec<u8>>,
}

impl VtHandler {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            responses: Vec::new(),
        }
    }

    fn handle_sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.grid.current_style = CellStyle::default();
            return;
        }

        let mut iter = params.iter();
        while let Some(sub) = iter.next() {
            let code = sub[0];
            match code {
                0 => self.grid.current_style = CellStyle::default(),
                1 => self.grid.current_style.bold = true,
                3 => self.grid.current_style.italic = true,
                4 => self.grid.current_style.underline = true,
                22 => self.grid.current_style.bold = false,
                23 => self.grid.current_style.italic = false,
                24 => self.grid.current_style.underline = false,
                30 => self.grid.current_style.fg = Some(Color::Black),
                31 => self.grid.current_style.fg = Some(Color::Red),
                32 => self.grid.current_style.fg = Some(Color::Green),
                33 => self.grid.current_style.fg = Some(Color::Yellow),
                34 => self.grid.current_style.fg = Some(Color::Blue),
                35 => self.grid.current_style.fg = Some(Color::Magenta),
                36 => self.grid.current_style.fg = Some(Color::Cyan),
                37 => self.grid.current_style.fg = Some(Color::White),
                39 => self.grid.current_style.fg = None,
                40 => self.grid.current_style.bg = Some(Color::Black),
                41 => self.grid.current_style.bg = Some(Color::Red),
                42 => self.grid.current_style.bg = Some(Color::Green),
                43 => self.grid.current_style.bg = Some(Color::Yellow),
                44 => self.grid.current_style.bg = Some(Color::Blue),
                45 => self.grid.current_style.bg = Some(Color::Magenta),
                46 => self.grid.current_style.bg = Some(Color::Cyan),
                47 => self.grid.current_style.bg = Some(Color::White),
                49 => self.grid.current_style.bg = None,
                90 => self.grid.current_style.fg = Some(Color::BrightBlack),
                91 => self.grid.current_style.fg = Some(Color::BrightRed),
                92 => self.grid.current_style.fg = Some(Color::BrightGreen),
                93 => self.grid.current_style.fg = Some(Color::BrightYellow),
                94 => self.grid.current_style.fg = Some(Color::BrightBlue),
                95 => self.grid.current_style.fg = Some(Color::BrightMagenta),
                96 => self.grid.current_style.fg = Some(Color::BrightCyan),
                97 => self.grid.current_style.fg = Some(Color::BrightWhite),
                100 => self.grid.current_style.bg = Some(Color::BrightBlack),
                101 => self.grid.current_style.bg = Some(Color::BrightRed),
                102 => self.grid.current_style.bg = Some(Color::BrightGreen),
                103 => self.grid.current_style.bg = Some(Color::BrightYellow),
                104 => self.grid.current_style.bg = Some(Color::BrightBlue),
                105 => self.grid.current_style.bg = Some(Color::BrightMagenta),
                106 => self.grid.current_style.bg = Some(Color::BrightCyan),
                107 => self.grid.current_style.bg = Some(Color::BrightWhite),
                38 => {
                    if sub.len() >= 5 && sub[1] == 2 {
                        let (r, g, b) = if sub.len() >= 6 {
                            (sub[3] as u8, sub[4] as u8, sub[5] as u8)
                        } else {
                            (sub[2] as u8, sub[3] as u8, sub[4] as u8)
                        };
                        self.grid.current_style.fg = Some(Color::Rgb(r, g, b));
                    } else if sub.len() >= 3 && sub[1] == 5 {
                        self.grid.current_style.fg = Some(Color::Indexed(sub[2] as u8));
                    } else if let Some(sub2) = iter.next() {
                        if sub2[0] == 5 {
                            if let Some(c) = iter.next() {
                                self.grid.current_style.fg = Some(Color::Indexed(c[0] as u8));
                            }
                        } else if sub2[0] == 2 {
                            if let (Some(r), Some(g), Some(b)) = (iter.next(), iter.next(), iter.next()) {
                                self.grid.current_style.fg =
                                    Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
                            }
                        }
                    }
                }
                48 => {
                    if sub.len() >= 5 && sub[1] == 2 {
                        let (r, g, b) = if sub.len() >= 6 {
                            (sub[3] as u8, sub[4] as u8, sub[5] as u8)
                        } else {
                            (sub[2] as u8, sub[3] as u8, sub[4] as u8)
                        };
                        self.grid.current_style.bg = Some(Color::Rgb(r, g, b));
                    } else if sub.len() >= 3 && sub[1] == 5 {
                        self.grid.current_style.bg = Some(Color::Indexed(sub[2] as u8));
                    } else if let Some(sub2) = iter.next() {
                        if sub2[0] == 5 {
                            if let Some(c) = iter.next() {
                                self.grid.current_style.bg = Some(Color::Indexed(c[0] as u8));
                            }
                        } else if sub2[0] == 2 {
                            if let (Some(r), Some(g), Some(b)) = (iter.next(), iter.next(), iter.next()) {
                                self.grid.current_style.bg =
                                    Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Perform for VtHandler {
    fn print(&mut self, c: char) {
        self.grid.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.grid.newline(),
            b'\r' => self.grid.carriage_return(),
            0x08 => self.grid.backspace(),
            b'\t' => {
                let next_tab = (self.grid.cursor.x / 8 + 1) * 8;
                self.grid.cursor.x = next_tab.min(self.grid.cols.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            'H' | 'f' => {
                let row = params.iter().next().map(|p| p[0] as usize).unwrap_or(1);
                let col = params.iter().nth(1).map(|p| p[0] as usize).unwrap_or(1);
                let y = row.saturating_sub(1);
                let x = col.saturating_sub(1);
                self.grid.move_cursor(x, y);
            }
            'A' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.cursor.y = self.grid.cursor.y.saturating_sub(count);
            }
            'B' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.cursor.y = (self.grid.cursor.y + count).min(self.grid.rows.saturating_sub(1));
            }
            'C' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.cursor.x = (self.grid.cursor.x + count).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.cursor.x = self.grid.cursor.x.saturating_sub(count);
            }
            'J' => {
                let mode = params.iter().next().map(|p| p[0] as usize).unwrap_or(0);
                self.grid.clear_screen(mode);
            }
            'K' => {
                let mode = params.iter().next().map(|p| p[0] as usize).unwrap_or(0);
                self.grid.clear_line(mode);
            }
            'm' => self.handle_sgr(params),
            's' => {
                self.grid.saved_cursor = Some(self.grid.cursor);
            }
            'u' => {
                if let Some(c) = self.grid.saved_cursor {
                    self.grid.cursor = c;
                }
            }
            'n' => {
                let code = params.iter().next().map(|p| p[0]).unwrap_or(0);
                if code == 6 {
                    // Cursor Position Report (DSR): ESC [ row ; col R
                    let r = self.grid.cursor.y + 1;
                    let c = self.grid.cursor.x + 1;
                    self.responses.push(format!("\x1b[{};{}R", r, c).into_bytes());
                } else if code == 5 {
                    // Terminal status OK
                    self.responses.push(b"\x1b[0n".to_vec());
                }
            }
            'c' => {
                if _intermediates == [b'>'] {
                    // Secondary Device Attributes
                    self.responses.push(b"\x1b[>0;10;0c".to_vec());
                } else {
                    // Primary Device Attributes (VT220 compatible)
                    self.responses.push(b"\x1b[?62;c".to_vec());
                }
            }
            'h' => {
                if _intermediates == [b'?'] {
                    handle_mode(&mut self.grid, params, true);
                }
            }
            'l' => {
                if _intermediates == [b'?'] {
                    handle_mode(&mut self.grid, params, false);
                }
            }
            'r' => handle_set_margins(&mut self.grid, params),
            'S' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.scroll_up(count);
            }
            'T' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.scroll_down(count);
            }
            'M' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.delete_lines(count);
            }
            'L' => {
                let count = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
                self.grid.insert_lines(count);
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if !params.is_empty() && params[0] == b"11" {
            if params.get(1).map_or(false, |p| *p == b"?") {
                // Respond to background color query with default dark background
                self.responses.push(b"\x1b]11;rgb:0000/0000/0000\x1b\\".to_vec());
            }
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        if byte == b'M' {
            if self.grid.cursor.y <= self.grid.scroll_top {
                self.grid.scroll_down(1);
            } else {
                self.grid.cursor.y = self.grid.cursor.y.saturating_sub(1);
            }
        }
    }
}

fn handle_set_margins(grid: &mut Grid, params: &Params) {
    let top = params.iter().next().map(|p| p[0] as usize).unwrap_or(1).max(1);
    let bottom = params.iter().nth(1).map(|p| p[0] as usize).unwrap_or(grid.rows);
    let top = top.saturating_sub(1);
    let bottom = bottom.saturating_sub(1).min(grid.rows.saturating_sub(1));
    if top < bottom {
        grid.scroll_top = top;
        grid.scroll_bottom = bottom;
    }
    grid.move_cursor(0, 0);
}

fn handle_mode(grid: &mut Grid, params: &Params, set: bool) {
    for param in params.iter() {
        match param[0] {
            25 => grid.cursor.visible = set,
            1049 | 1047 | 47 => grid.clear_screen(2),
            _ => {}
        }
    }
}

pub struct VtScreen {
    parser: vte::Parser,
    handler: VtHandler,
}

impl VtScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            handler: VtHandler::new(cols, rows),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.handler, bytes);
    }

    pub fn take_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.handler.responses)
    }

    pub fn snapshot(&self) -> String {
        self.handler.grid.render_plain()
    }

    pub fn grid(&self) -> &Grid {
        &self.handler.grid
    }

    pub fn grid_mut(&mut self) -> &mut Grid {
        &mut self.handler.grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vt_plain_text() {
        let mut vt = VtScreen::new(20, 5);
        vt.feed(b"Hello from MPX\r\nSecond line");
        let snap = vt.snapshot();
        assert!(snap.contains("Hello from MPX"));
        assert!(snap.contains("Second line"));
    }

    #[test]
    fn test_vt_cursor_positioning() {
        let mut vt = VtScreen::new(20, 5);
        // Move cursor to row 2, col 5 (1-based: ESC[2;5H) and print 'X'
        vt.feed(b"\x1b[2;5HX");
        assert_eq!(vt.grid().cursor.x, 5);
        assert_eq!(vt.grid().cursor.y, 1);
        assert_eq!(vt.grid().get_cell(4, 1).unwrap().c, 'X');
    }

    #[test]
    fn test_vt_colors() {
        let mut vt = VtScreen::new(20, 5);
        // Red bold text: ESC[1;31mREDESC[0m
        vt.feed(b"\x1b[1;31mRED\x1b[0m");
        let cell = vt.grid().get_cell(0, 0).unwrap();
        assert_eq!(cell.c, 'R');
        assert_eq!(cell.style.fg, Some(Color::Red));
        assert!(cell.style.bold);

        // Reset style
        assert_eq!(vt.grid().current_style, CellStyle::default());
    }

    #[test]
    fn test_vt_terminal_queries() {
        let mut vt = VtScreen::new(20, 5);
        // Move to row 3, col 7 and query DSR (ESC[6n)
        vt.feed(b"\x1b[3;7H\x1b[6n");
        let resps = vt.take_responses();
        assert_eq!(resps.len(), 1);
        assert_eq!(resps[0], b"\x1b[3;7R");

        // Query primary Device Attributes (ESC[c)
        vt.feed(b"\x1b[c");
        let resps2 = vt.take_responses();
        assert_eq!(resps2.len(), 1);
        assert_eq!(resps2[0], b"\x1b[?62;c");
    }

    #[test]
    fn test_vt_scrolling_region_and_commands() {
        let mut vt = VtScreen::new(20, 5);
        vt.feed(b"\x1b[2;4r");
        assert_eq!(vt.grid().scroll_top, 1);
        assert_eq!(vt.grid().scroll_bottom, 3);

        vt.feed(b"\x1b[2;1Hline2");
        assert_eq!(vt.grid().get_cell(0, 1).unwrap().c, 'l');

        vt.feed(b"\x1b[1T");
        assert_eq!(vt.grid().get_cell(0, 2).unwrap().c, 'l');
    }
}
