use std::borrow::Cow;
use vte::{Params, Perform};

use super::grid::{Cell, CellStyle, Color, Grid};
use crate::keys::TermModes;

pub struct VtHandler {
    pub grid: Grid,
    pub responses: Vec<Vec<u8>>,
    pub passthroughs: Vec<Vec<u8>>,
    pub bg_color: Option<String>,
    modes: TermModes,
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
            title: None,
            alerts: Vec::new(),
            last_char: None,
            dcs_buf: None,
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
                2 => self.grid.current_style.faint = true,
                3 => self.grid.current_style.italic = true,
                4 => self.grid.current_style.underline = sub.get(1) != Some(&0),
                21 => self.grid.current_style.underline = true,
                7 => self.grid.current_style.reverse = true,
                22 => {
                    self.grid.current_style.bold = false;
                    self.grid.current_style.faint = false;
                }
                23 => self.grid.current_style.italic = false,
                24 => self.grid.current_style.underline = false,
                27 => self.grid.current_style.reverse = false,
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
                        } else if sub2[0] == 2
                            && let (Some(r), Some(g), Some(b)) =
                                (iter.next(), iter.next(), iter.next())
                        {
                            self.grid.current_style.fg =
                                Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
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
                        } else if sub2[0] == 2
                            && let (Some(r), Some(g), Some(b)) =
                                (iter.next(), iter.next(), iter.next())
                        {
                            self.grid.current_style.bg =
                                Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8));
                        }
                    }
                }
                58 => skip_extended_color(sub, &mut iter),
                _ => {}
            }
        }
    }

    fn handle_mode(&mut self, params: &Params, set: bool) {
        for param in params.iter() {
            match param[0] {
                1 => self.modes.app_cursor = set,
                25 => self.grid.cursor.visible = set,
                1049 | 1047 | 47 => self.grid.clear_screen(2),
                69 => self.grid.set_lr_margins_mode(set),
                1000 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::X10
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1002 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::ButtonEvent
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1003 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::AnyEvent
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1006 => self.modes.mouse_sgr = set,
                1004 => self.modes.focus_events = set,
                2004 => self.modes.bracketed_paste = set,
                _ => {}
            }
        }
    }

    fn push_notify(&mut self, text: String) {
        if !text.is_empty() {
            self.alerts.push(Alert::Notify(text));
        }
    }

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
        self.title = None;
        self.bg_color = None;
        self.dcs_buf = None;
        self.last_char = None;
    }

    fn handle_csi_cursor_edit(&mut self, action: char, params: &Params) {
        let n = count_param(params);
        match action {
            'G' | '`' => {
                let y = self.grid.cursor.y;
                self.grid.move_cursor(n - 1, y);
            }
            'd' => {
                let x = self.grid.cursor.x;
                self.grid.move_cursor(x, n - 1);
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
            'Z' => {
                for _ in 0..n {
                    self.grid.cursor.x = back_tab(self.grid.cursor.x);
                }
            }
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
}

/// Consumes the arguments of an extended colour (`58;5;n`, `58;2;r;g;b`) whose value is unused.
fn skip_extended_color<'a>(sub: &[u16], iter: &mut impl Iterator<Item = &'a [u16]>) {
    if sub.len() > 1 {
        return;
    }
    match iter.next().map(|s| s[0]) {
        Some(5) => {
            iter.next();
        }
        Some(2) => {
            for _ in 0..3 {
                iter.next();
            }
        }
        _ => {}
    }
}

const MAX_DCS_LEN: usize = 1024 * 1024;

fn unescape_tmux_dcs(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut i = 0;
    while i < payload.len() {
        if payload[i] == 0x1b && i + 1 < payload.len() && payload[i + 1] == 0x1b {
            out.push(0x1b);
            i += 2;
        } else {
            out.push(payload[i]);
            i += 1;
        }
    }
    out
}

fn format_osc(params: &[&[u8]], bell_terminated: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(b"\x1b]");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push(b';');
        }
        buf.extend_from_slice(p);
    }
    if bell_terminated {
        buf.push(0x07);
    } else {
        buf.extend_from_slice(b"\x1b\\");
    }
    buf
}

fn join_osc(parts: &[&[u8]]) -> String {
    parts
        .iter()
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(";")
}

fn count_param(params: &Params) -> usize {
    params
        .iter()
        .next()
        .map(|p| p[0] as usize)
        .unwrap_or(1)
        .max(1)
}

fn back_tab(x: usize) -> usize {
    x.saturating_sub(1) / 8 * 8
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
        // `CSI > 4;2 m` (modifyOtherKeys), `CSI > 1 u` (kitty keyboard), `CSI ? 6 n` and similar
        // private forms share finals with SGR, cursor restore and DSR but mean something else.
        if !intermediates.is_empty() && !matches!(action, 'c' | 'h' | 'l') {
            return;
        }
        match action {
            'H' | 'f' => {
                let row = params.iter().next().map(|p| p[0] as usize).unwrap_or(1);
                let col = params.iter().nth(1).map(|p| p[0] as usize).unwrap_or(1);
                let y = row.saturating_sub(1);
                let x = col.saturating_sub(1);
                self.grid.move_cursor(x, y);
            }
            'A' => self.cursor_up(count_param(params)),
            'B' => self.cursor_down(count_param(params)),
            'C' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.cursor.x =
                    (self.grid.cursor.x + count).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
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
            // With DECLRMM set, `CSI s` is DECSLRM (left/right margins), not save-cursor.
            's' if self.grid.lr_margins => {
                let value = |i: usize| {
                    params
                        .iter()
                        .nth(i)
                        .map(|p| p[0] as usize)
                        .filter(|&v| v > 0)
                };
                self.grid.set_lr_margins(value(0), value(1));
            }
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
                    self.responses
                        .push(format!("\x1b[{};{}R", r, c).into_bytes());
                } else if code == 5 {
                    // Terminal status OK
                    self.responses.push(b"\x1b[0n".to_vec());
                }
            }
            'c' => {
                if intermediates == b">" {
                    // Secondary Device Attributes
                    self.responses.push(b"\x1b[>0;10;0c".to_vec());
                } else {
                    // Primary Device Attributes (VT220 compatible)
                    self.responses.push(b"\x1b[?62;c".to_vec());
                }
            }
            'h' => {
                if intermediates == b"?" {
                    self.handle_mode(params, true);
                }
            }
            'l' => {
                if intermediates == b"?" {
                    self.handle_mode(params, false);
                }
            }
            'r' => handle_set_margins(&mut self.grid, params),
            'S' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.scroll_up(count);
            }
            'T' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.scroll_down(count);
            }
            'M' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.delete_lines(count);
            }
            'L' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.insert_lines(count);
            }
            _ => self.handle_csi_cursor_edit(action, params),
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let mut buf = Vec::new();
        buf.push(action as u8);
        self.dcs_buf = Some(buf);
    }

    fn put(&mut self, byte: u8) {
        if let Some(buf) = &mut self.dcs_buf {
            if buf.len() < MAX_DCS_LEN {
                buf.push(byte);
            }
        }
    }

    fn unhook(&mut self) {
        if let Some(buf) = self.dcs_buf.take() {
            let payload = buf
                .strip_prefix(b"tmux;")
                .or_else(|| buf.strip_prefix(b"mpx;"));
            if let Some(payload) = payload {
                let unescaped = unescape_tmux_dcs(payload);
                if !unescaped.is_empty() {
                    self.passthroughs.push(unescaped);
                }
            }
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        match params {
            [b"0" | b"2", rest @ ..] => self.title = Some(join_osc(rest)),
            // OSC 9;4 is ConEmu/Windows Terminal progress reporting, not a notification.
            [b"9", b"4", ..] => {}
            [b"9", rest @ ..] => self.push_notify(join_osc(rest)),
            [b"777", b"notify", title, body @ ..] => {
                let title = String::from_utf8_lossy(title).into_owned();
                let body = join_osc(body);
                let text = if body.is_empty() {
                    title
                } else {
                    format!("{title}: {body}")
                };
                self.push_notify(text);
            }
            // Respond to background color query
            [b"11", b"?", ..] => {
                let bg = self.bg_color.as_deref().unwrap_or("rgb:0000/0000/0000");
                self.responses
                    .push(format!("\x1b]11;{bg}\x1b\\").into_bytes());
            }
            // Background color set: save for queries and pass through
            [b"11", color, ..] => {
                self.bg_color = Some(String::from_utf8_lossy(color).into_owned());
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            // Palette (4), Foreground (10), Cursor (12), Clipboard (52), Resets (104, 110, 111, 112)
            [b"4" | b"10" | b"12" | b"52" | b"104" | b"110" | b"111" | b"112", ..] => {
                if params[0] == b"111" {
                    self.bg_color = None;
                }
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            // iTerm2 custom sequences (e.g. OSC Pg... for colors)
            [first, ..] if first.starts_with(b"P") => {
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            _ => {}
        }
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

fn handle_set_margins(grid: &mut Grid, params: &Params) {
    let top = params
        .iter()
        .next()
        .map(|p| p[0] as usize)
        .unwrap_or(1)
        .max(1);
    let bottom = params
        .iter()
        .nth(1)
        .map(|p| p[0] as usize)
        .unwrap_or(grid.rows);
    let top = top.saturating_sub(1);
    let bottom = bottom.saturating_sub(1).min(grid.rows.saturating_sub(1));
    if top < bottom {
        grid.scroll_top = top;
        grid.scroll_bottom = bottom;
    }
    grid.move_cursor(0, 0);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    Bell,
    Notify(String),
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

    /// A screen that keeps no scrollback: what a live pane shows. Scrollback lives in the pane's
    /// retained output and is rebuilt from it on demand, so floods cost no history work.
    pub fn live(cols: usize, rows: usize) -> Self {
        let mut screen = Self::new(cols, rows);
        screen.grid_mut().set_history_limit(0);
        screen
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

    /// Input modes the program requested (DECCKM, bracketed paste, focus events).
    pub fn modes(&self) -> TermModes {
        self.handler.modes
    }

    /// Takes on input modes set before the output this screen was fed, which it could not see.
    pub fn set_modes(&mut self, modes: TermModes) {
        self.handler.modes = modes;
    }

    /// Latest OSC 0/2 window title.
    pub fn title(&self) -> Option<&str> {
        self.handler.title.as_deref()
    }

    /// Drains BEL and OSC 9 / OSC 777 notifications received since the last call.
    pub fn take_alerts(&mut self) -> Vec<Alert> {
        std::mem::take(&mut self.handler.alerts)
    }

    /// Drains passthrough sequences (DCS tmux, OSC 4/10/11/12/52) received since the last call.
    pub fn take_passthroughs(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.handler.passthroughs)
    }

    /// Latest background color set via OSC 11.
    pub fn bg_color(&self) -> Option<&str> {
        self.handler.bg_color.as_deref()
    }

    /// Scrollback history plus the visible screen as plain text, trailing blank lines trimmed;
    /// `tail` keeps only the last N lines.
    pub fn scrollback_text(&self, tail: Option<usize>) -> String {
        if tail == Some(0) {
            return String::new();
        }
        let grid = &self.handler.grid;
        let all_rows = grid
            .history
            .iter()
            .map(|row| Cow::Owned(row.to_cells(0)))
            .chain(grid.screen_lines().map(Cow::Borrowed));

        let non_blank_from_end = all_rows.rev().skip_while(|row| is_blank_row(row));

        let rows: Vec<Cow<'_, [Cell]>> = match tail {
            Some(n) => non_blank_from_end.take(n).collect(),
            None => non_blank_from_end.collect(),
        };

        let lines: Vec<String> = rows.iter().rev().map(|row| row_text(row)).collect();
        lines.join("\n")
    }
}

fn is_blank_row(row: &[Cell]) -> bool {
    row.iter().all(|cell| cell.c.is_whitespace())
}

fn row_text(row: &[Cell]) -> String {
    row.iter()
        .map(|cell| cell.c)
        .collect::<String>()
        .trim_end()
        .to_owned()
}
